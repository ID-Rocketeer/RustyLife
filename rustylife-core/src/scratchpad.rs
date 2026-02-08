use crate::hash::hash_coordinates;
use std::cell::UnsafeCell;

/// A coordinate candidate for cell creation.
/// Carrying the hash and coordinates together avoids re-hashing in Phase 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Candidate {
    pub hash_idx: usize,
    pub x: i128,
    pub y: i128,
}

/// aligned to 128 bytes to prevent false sharing on both 64-byte (x86) and 128-byte (some ARM) cache lines.
#[repr(align(128))]
struct CachePadded<T> {
    value: T,
}

impl<T> CachePadded<T> {
    fn new(value: T) -> Self {
        Self { value }
    }
}

/// A transient, grid-based scratchpad for coordinate identification.
/// Organized as `[thread_idx][bucket_idx]` to ensure zero-contention writes in Phase 1.
pub struct Scratchpad {
    /// Inner structure: `Vec<ThreadRow>` where `ThreadRow` is `Vec<BucketBuffer>`
    /// `rows[thread_idx][bucket_idx] -> CachePadded<UnsafeCell<Vec<Candidate>>>`
    rows: Vec<Vec<CachePadded<UnsafeCell<Vec<Candidate>>>>>,
    thread_count: usize,
    bucket_count: usize,
}

impl Scratchpad {
    pub fn new(thread_count: usize, bucket_count: usize) -> Self {
        let mut rows = Vec::with_capacity(thread_count);
        for _ in 0..thread_count {
            let mut row = Vec::with_capacity(bucket_count);
            for _ in 0..bucket_count {
                row.push(CachePadded::new(UnsafeCell::new(Vec::with_capacity(128))));
            }
            rows.push(row);
        }
        Self {
            rows,
            thread_count,
            bucket_count,
        }
    }

    /// Push a candidate into the scratchpad.
    /// SAFETY: thread_idx must be unique to the calling thread for the duration of its use.
    /// The caller must ensure no other thread is accessing the same thread_idx.
    pub fn push_candidate(&self, thread_idx: usize, x: i128, y: i128) {
        let hash_idx = hash_coordinates(x, y, self.bucket_count);
        unsafe {
            // Access .value inside the padded wrapper
            let vec_ptr = self.rows[thread_idx][hash_idx].value.get();
            (*vec_ptr).push(Candidate { hash_idx, x, y });
        }
    }

    /// Clears all buffers without deallocating.
    /// SAFETY: Must be called when no other threads are accessing the scratchpad.
    pub fn clear(&self) {
        for row in &self.rows {
            for bucket_cell in row {
                unsafe {
                    (*bucket_cell.value.get()).clear();
                }
            }
        }
    }

    /// Collects all candidates for a specific bucket across all threads into a provided buffer.
    /// This is the "Gather" step of the pipeline.
    /// SAFETY: Must be called when no other threads are writing to the scratchpad.
    pub fn get_column_into(&self, bucket_idx: usize, out: &mut Vec<Candidate>) {
        out.clear();
        for thread_idx in 0..self.thread_count {
            unsafe {
                let vec_ptr = self.rows[thread_idx][bucket_idx].value.get();
                out.append(&mut *vec_ptr);
            }
        }
    }

    /// Obsolete: use get_column_into to avoid allocations.
    pub fn get_column(&self, bucket_idx: usize) -> Vec<Candidate> {
        let mut column = Vec::new();
        self.get_column_into(bucket_idx, &mut column);
        column
    }

    pub fn bucket_count(&self) -> usize {
        self.bucket_count
    }
}

// Scratchpad is Send/Sync because we manage the safety of individual row access
// via the engine's phase barriers and unique thread indexing.
unsafe impl Send for Scratchpad {}
unsafe impl Sync for Scratchpad {}
