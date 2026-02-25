use crate::hash::hash_coordinates;
use std::cell::UnsafeCell;

/// A coordinate candidate for cell creation.
/// Carrying the hash and coordinates together avoids re-hashing in Phase 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Candidate {
    pub hash_idx: usize,
    pub x: i128,
    pub y: i128,
    pub payload: u64,
    pub mask: u8,
}

/// aligned to 128 bytes to prevent false sharing on both 64-byte (x86) and 128-byte (some ARM) cache lines.
#[repr(align(128))]
pub struct CachePadded<T> {
    pub value: T,
}

impl<T> CachePadded<T> {
    pub fn new(value: T) -> Self {
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
    pub fn push_candidate(&self, thread_idx: usize, x: i128, y: i128, payload: u64, mask: u8) {
        let hash_idx = hash_coordinates(x, y, self.bucket_count);
        unsafe {
            // Access .value inside the padded wrapper
            let vec_ptr = self.rows[thread_idx][hash_idx].value.get();
            (*vec_ptr).push(Candidate {
                hash_idx,
                x,
                y,
                payload,
                mask,
            });
        }
    }

    /// Clears all buffers without deallocating wholesale, but applying staggered
    /// capacity shrinking to prevent memory hoarding on high thread counts.
    /// SAFETY: Must be called when no other threads are accessing the scratchpad.
    pub fn clear(&self, generation: u64) {
        for (thread_idx, row) in self.rows.iter().enumerate() {
            // Stagger the shrinking: only one thread checks its buckets per generation.
            // This prevents a massive latency spike from all threads reallocating at once.
            let should_shrink = (generation as usize % self.thread_count) == thread_idx;

            for bucket_cell in row {
                unsafe {
                    let vec = &mut *bucket_cell.value.get();
                    if should_shrink && vec.capacity() > 16384 && vec.len() < 4096 {
                        vec.shrink_to_fit();
                    }
                    vec.clear();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_staggered_capacity_shrink() {
        let thread_count = 4;
        let bucket_count = 2;
        let scratchpad = Scratchpad::new(thread_count, bucket_count);

        // Populate to exceed shrink threshold (16384 capacity, <4096 len at clear time)
        for t in 0..thread_count {
            for b in 0..bucket_count {
                unsafe {
                    let vec_ptr = scratchpad.rows[t][b].value.get();
                    (*vec_ptr).reserve_exact(20000);
                    (*vec_ptr).push(Candidate {
                        hash_idx: b,
                        x: 0,
                        y: 0,
                        payload: 0,
                        mask: 0,
                    });
                }
            }
        }

        // Verify initial capacity
        for t in 0..thread_count {
            for b in 0..bucket_count {
                unsafe {
                    let vec_ptr = scratchpad.rows[t][b].value.get();
                    assert!((*vec_ptr).capacity() >= 20000);
                    assert_eq!((*vec_ptr).len(), 1);
                }
            }
        }

        // Gen 0 shrinks thread 0
        scratchpad.clear(0);
        for t in 0..thread_count {
            for b in 0..bucket_count {
                unsafe {
                    let vec_ptr = scratchpad.rows[t][b].value.get();
                    assert_eq!((*vec_ptr).len(), 0);
                    if t == 0 {
                        assert!((*vec_ptr).capacity() < 20000, "Thread 0 should have shrunk");
                        // shrink_to_fit on a len=1 vector shrinks it to capacity >= 1.
                        assert!(
                            (*vec_ptr).capacity() <= 4,
                            "Vector should be shrunk to fit its 1 element"
                        );
                    } else {
                        assert!(
                            (*vec_ptr).capacity() >= 20000,
                            "Thread {} should NOT have shrunk yet",
                            t
                        );
                    }
                }
            }
        }

        // Gen 1 shrinks thread 1
        scratchpad.clear(1);
        for t in 0..thread_count {
            for b in 0..bucket_count {
                unsafe {
                    let vec_ptr = scratchpad.rows[t][b].value.get();
                    assert_eq!((*vec_ptr).len(), 0);
                    if t == 0 || t == 1 {
                        assert!(
                            (*vec_ptr).capacity() < 20000,
                            "Thread {} should have shrunk",
                            t
                        );
                    } else {
                        assert!(
                            (*vec_ptr).capacity() >= 20000,
                            "Thread {} should NOT have shrunk yet",
                            t
                        );
                    }
                }
            }
        }

        // Gen 4 shrinks thread 0 again (modulo wrap around)
        // Let's reserve thread 0 again and verify Gen 4 shrinks it
        for b in 0..bucket_count {
            unsafe {
                let vec_ptr = scratchpad.rows[0][b].value.get();
                (*vec_ptr).reserve_exact(20000);
                assert!((*vec_ptr).capacity() >= 20000);
            }
        }

        scratchpad.clear(4);
        for b in 0..bucket_count {
            unsafe {
                let vec_ptr = scratchpad.rows[0][b].value.get();
                assert!(
                    (*vec_ptr).capacity() < 20000,
                    "Thread 0 should have shrunk again on gen 4"
                );
            }
        }
    }
}
