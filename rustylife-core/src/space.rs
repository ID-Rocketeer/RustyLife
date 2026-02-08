use crate::cell::Cell;
use crate::hash::hash_coordinates;
use crate::state::{MaskGuard, SimulationMasks};
use crate::tree::CellTree;
use std::cell::UnsafeCell;
use std::io::Write;

use std::sync::RwLock;

/// Sparse storage for cells using a configurable number of buckets.
///
/// Each bucket contains a binary search tree (CellTree) of cells, allowing
/// for efficient lookup and concurrent access.
pub struct SparseStorage {
    /// The collection of cell buckets.
    pub buckets: Box<[RwLock<CellTree>]>,
}

impl SparseStorage {
    pub fn new(bucket_count: usize) -> Self {
        let buckets = (0..bucket_count)
            .map(|_| RwLock::new(CellTree::new()))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self { buckets }
    }

    pub fn insert(&self, cell: Cell) {
        let coords = cell.coordinates();
        let idx = hash_coordinates(coords.0, coords.1, self.buckets.len());
        self.buckets[idx]
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(cell);
    }

    pub fn find_and_apply<F, R>(&self, x: i128, y: i128, f: F) -> Option<R>
    where
        F: FnOnce(&Cell) -> R,
    {
        let idx = hash_coordinates(x, y, self.buckets.len());
        self.buckets[idx]
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .find_and_apply((x, y), f)
    }

    pub fn find_or_create_and_apply<F, C, R>(&self, x: i128, y: i128, creator: C, f: F) -> R
    where
        C: FnOnce() -> Cell,
        F: FnOnce(&mut Cell) -> R,
    {
        let idx = hash_coordinates(x, y, self.buckets.len());
        self.buckets[idx]
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .find_or_create_and_apply((x, y), creator, f)
    }

    pub fn collect_all(
        &self,
        current_mask: usize,
        last_mask: usize,
        last_last_mask: usize,
        out: &mut Vec<((i128, i128), u8)>,
    ) {
        for bucket in &self.buckets {
            bucket
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .collect_all(current_mask, last_mask, last_last_mask, out);
        }
    }

    pub fn collect_in_rect(
        &self,
        min: (i128, i128),
        max: (i128, i128),
        current_mask: usize,
        last_mask: usize,
        last_last_mask: usize,
        out: &mut Vec<((i128, i128), u8)>,
    ) {
        for bucket in &self.buckets {
            bucket
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .collect_in_rect(min, max, current_mask, last_mask, last_last_mask, out);
        }
    }

    pub fn clear(&self) {
        for bucket in self.buckets.iter() {
            bucket.write().unwrap_or_else(|e| e.into_inner()).clear();
        }
    }

    pub fn write_cells_streaming(
        &self,
        current_mask: usize,
        last_mask: usize,
        last_last_mask: usize,
        buffered_writer: &mut std::io::BufWriter<std::fs::File>,
        hasher: &mut crc32fast::Hasher,
    ) -> std::io::Result<()> {
        for bucket in &self.buckets {
            bucket
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .write_cells_streaming(
                    current_mask,
                    last_mask,
                    last_last_mask,
                    buffered_writer,
                    hasher,
                )?;
        }
        Ok(())
    }
}

/// A high-level representation of the simulation grid.
///
/// `SimulationSpace` coordinates cell storage and simulation mask management.
/// It uses interior mutability to allow concurrent access during controlled
/// simulation phases.
pub struct SimulationSpace {
    /// The global simulation masks governing state transitions.
    pub mask: SimulationMasks,
    storage: UnsafeCell<SparseStorage>,
}

impl SimulationSpace {
    pub fn new(bucket_count: usize) -> Self {
        Self {
            mask: SimulationMasks::new(),
            storage: UnsafeCell::new(SparseStorage::new(bucket_count)),
        }
    }

    pub fn storage(&self) -> &SparseStorage {
        unsafe { &*self.storage.get() }
    }

    pub fn storage_raw(&self) -> *mut SparseStorage {
        self.storage.get()
    }

    /// SAFETY: Must only be called during a gated phase where no other thread
    /// is accessing the storage.
    pub unsafe fn storage_mut(&self) -> &mut SparseStorage {
        unsafe { &mut *self.storage.get() }
    }

    /// Aquires a shared read lock for the space.
    pub fn read(&self) -> MaskGuard<'_> {
        self.mask.read()
    }

    /// Acquires an exclusive write lock to advance the space's global mask.
    pub fn advance_generation(&self) {
        self.mask.cycle()
    }

    pub fn collect_all_states(&self) -> Vec<((i128, i128), u8)> {
        let guard = self.mask.read();
        let current_mask = guard.current_state_mask();
        let last_mask = guard.last_state_mask();
        let last_last_mask = guard.next_state_mask();

        let mut all = Vec::new();
        self.storage()
            .collect_all(current_mask, last_mask, last_last_mask, &mut all);
        all
    }

    /// Clears the entire simulation space.
    pub fn clear(&self) {
        self.storage().clear();
        self.mask.reset();
    }

    /// Repairs a tainted simulation state by resetting neighbor counts.
    /// Used when recovering from an aggressive stop.
    pub fn repair(&self) {
        for bucket in self.storage().buckets.iter() {
            bucket
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .reset_all_counts();
        }
    }

    pub fn collect_in_rect(
        &self,
        min: (i128, i128),
        max: (i128, i128),
        out: &mut Vec<((i128, i128), u8)>,
    ) {
        let guard = self.mask.read();
        let curr = guard.current_state_mask();
        let last = guard.last_state_mask();
        let next = guard.next_state_mask();
        self.storage()
            .collect_in_rect(min, max, curr, last, next, out);
    }

    /// Seeds a glider pattern at the specified coordinates.
    pub fn seed_glider(&self, ox: i128, oy: i128) {
        let guard = self.mask.read();
        let mask = guard.current_state_mask();
        let pts = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
        for (px, py) in pts {
            self.storage().insert(Cell::new(
                ox + px,
                oy + py,
                crate::cell::CellState::Alive,
                mask,
            ));
        }
    }

    /// Seeds a blinker pattern at the specified coordinates.
    pub fn seed_blinker(&self, ox: i128, oy: i128) {
        let guard = self.mask.read();
        let mask = guard.current_state_mask();
        let pts = [(0, 0), (1, 0), (2, 0)];
        for (px, py) in pts {
            self.storage().insert(Cell::new(
                ox + px,
                oy + py,
                crate::cell::CellState::Alive,
                mask,
            ));
        }
    }

    /// Seeds an R-pentomino pattern at the specified coordinates.
    pub fn seed_r_pentomino(&self, ox: i128, oy: i128) {
        let guard = self.mask.read();
        let mask = guard.current_state_mask();
        let pts = [(1, 0), (2, 0), (0, 1), (1, 1), (1, 2)];
        for (px, py) in pts {
            self.storage().insert(Cell::new(
                ox + px,
                oy + py,
                crate::cell::CellState::Alive,
                mask,
            ));
        }
    }

    /// Seeds a Goshen Glider Gun pattern.
    pub fn seed_glider_gun(&self, ox: i128, oy: i128) {
        let guard = self.mask.read();
        let mask = guard.current_state_mask();
        let pts = [
            (24, 0),
            (22, 1),
            (24, 1),
            (12, 2),
            (13, 2),
            (20, 2),
            (21, 2),
            (34, 2),
            (35, 2),
            (11, 3),
            (15, 3),
            (20, 3),
            (21, 3),
            (34, 3),
            (35, 3),
            (0, 4),
            (1, 4),
            (10, 4),
            (16, 4),
            (20, 4),
            (21, 4),
            (0, 5),
            (1, 5),
            (10, 5),
            (14, 5),
            (16, 5),
            (17, 5),
            (22, 5),
            (24, 5),
            (10, 6),
            (16, 6),
            (24, 6),
            (11, 7),
            (15, 7),
            (12, 8),
            (13, 8),
        ];
        for (px, py) in pts {
            self.storage().insert(Cell::new(
                ox + px,
                oy + py,
                crate::cell::CellState::Alive,
                mask,
            ));
        }
    }

    /// Seeds a lightweight spaceship (LWSS).
    pub fn seed_spaceship(&self, ox: i128, oy: i128) {
        let guard = self.mask.read();
        let mask = guard.current_state_mask();
        let pts = [
            (1, 0),
            (4, 0),
            (0, 1),
            (0, 2),
            (4, 2),
            (0, 3),
            (1, 3),
            (2, 3),
            (3, 3),
        ];
        for (px, py) in pts {
            self.storage().insert(Cell::new(
                ox + px,
                oy + py,
                crate::cell::CellState::Alive,
                mask,
            ));
        }
    }

    /// Seeds a block (stable).
    pub fn seed_block(&self, ox: i128, oy: i128) {
        let guard = self.mask.read();
        let mask = guard.current_state_mask();
        let pts = [(0, 0), (1, 0), (0, 1), (1, 1)];
        for (px, py) in pts {
            self.storage().insert(Cell::new(
                ox + px,
                oy + py,
                crate::cell::CellState::Alive,
                mask,
            ));
        }
    }

    /// Seeds a beehive (stable).
    pub fn seed_beehive(&self, ox: i128, oy: i128) {
        let guard = self.mask.read();
        let mask = guard.current_state_mask();
        let pts = [(1, 0), (2, 0), (0, 1), (3, 1), (1, 2), (2, 2)];
        for (px, py) in pts {
            self.storage().insert(Cell::new(
                ox + px,
                oy + py,
                crate::cell::CellState::Alive,
                mask,
            ));
        }
    }

    /// Seeds a pattern from a standard `.cells` format string.
    pub fn seed_from_cells(&self, ox: i128, oy: i128, content: &str) {
        let guard = self.mask.read();
        let mask = guard.current_state_mask();
        let mut y_offset = 0;
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('!') {
                continue;
            }
            for (x_offset, ch) in line.chars().enumerate() {
                if ch == 'O' || ch == '*' {
                    self.storage().insert(Cell::new(
                        ox + x_offset as i128,
                        oy + y_offset as i128,
                        crate::cell::CellState::Alive,
                        mask,
                    ));
                }
            }
            y_offset += 1;
        }
    }

    /// Seeds a pattern from a Run Length Encoded (RLE) string.
    pub fn seed_from_rle(&self, ox: i128, oy: i128, rle: &str) {
        let guard = self.mask.read();
        let mask = guard.current_state_mask();
        let mut x = 0;
        let mut y = 0;
        let mut start_x = 0;
        let mut num = 0;

        for line in rle.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Handle position lines: #P x y OR #R x y
            if line.starts_with("#P") || line.starts_with("#R") {
                let parts: Vec<&str> = line[2..].split_whitespace().collect();
                if parts.len() >= 2 {
                    if let (Ok(nx), Ok(ny)) = (parts[0].parse::<i128>(), parts[1].parse::<i128>()) {
                        x = nx;
                        y = ny;
                        start_x = nx;
                    }
                }
                continue;
            }

            if line.starts_with('#') || line.to_ascii_lowercase().starts_with("x =") {
                continue;
            }

            for ch in line.chars() {
                if ch.is_digit(10) {
                    num = num * 10 + ch.to_digit(10).unwrap() as i128;
                } else if ch == ' ' || ch == '\t' || ch == '\r' || ch == '\n' || ch == ',' {
                    // Skip whitespace and separators, do NOT reset num
                    continue;
                } else {
                    let count = if num == 0 { 1 } else { num };
                    num = 0;
                    match ch.to_ascii_lowercase() {
                        'b' => x += count,
                        'o' => {
                            for i in 0..count {
                                self.storage().insert(Cell::new(
                                    ox + x + i,
                                    oy + y,
                                    crate::cell::CellState::Alive,
                                    mask,
                                ));
                            }
                            x += count;
                        }
                        '$' => {
                            y += count;
                            x = start_x;
                        }
                        '!' => return,
                        _ => {} // Ignore unknown characters but consume num
                    }
                }
            }
        }
    }

    pub fn encode_to_file(
        &self,
        path: &std::path::Path,
        generation: u64,
        total_cells: u64,
        is_running: bool,
        record_count: u64,
    ) -> std::io::Result<()> {
        let guard = self.mask.read();
        self.encode_to_file_with_masks(
            path,
            generation,
            total_cells,
            is_running,
            record_count,
            guard.current_state_mask(),
            guard.last_state_mask(),
            guard.next_state_mask(),
        )
    }

    pub fn encode_to_file_with_masks(
        &self,
        path: &std::path::Path,
        generation: u64,
        total_cells: u64,
        is_running: bool,
        record_count: u64,
        current_mask: usize,
        last_mask: usize,
        next_mask: usize,
    ) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;
        let mut hasher = crc32fast::Hasher::new();

        // 1. Write Header: [gen: u64][total: u64][is_running: u8][count: u64]
        let mut header = Vec::with_capacity(25);
        header.extend_from_slice(&generation.to_le_bytes());
        header.extend_from_slice(&total_cells.to_le_bytes()); // survivors
        header.push(if is_running { 1 } else { 0 });
        header.extend_from_slice(&record_count.to_le_bytes()); // records in file

        hasher.update(&header);
        file.write_all(&header)?;

        // 2. Stream Cells from buckets
        let mut buffered_writer = std::io::BufWriter::new(file);
        self.storage().write_cells_streaming(
            current_mask,
            last_mask,
            next_mask,
            &mut buffered_writer,
            &mut hasher,
        )?;

        // Flush buffer back to file to append CRC
        let mut file = buffered_writer
            .into_inner()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        // 3. Append CRC32
        let crc = hasher.finalize();
        file.write_all(&crc.to_le_bytes())?;

        Ok(())
    }
}

// SAFETY: SimulationSpace is Sync because all concurrent access to the UnsafeCell<SparseStorage>
// is coordinated via the engine's phase barriers, ensuring no write-read or write-write overlaps.
unsafe impl Sync for SimulationSpace {}

impl SparseStorage {
    pub fn get_bucket_mut(&mut self, idx: usize) -> &mut CellTree {
        self.buckets[idx].get_mut().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::{Cell, CellState};

    #[test]
    fn test_sparse_storage_parallel_traits() {
        let storage = SparseStorage::new(crate::BUCKET_COUNT);
        let manager = SimulationMasks::new();
        let guard = manager.read();

        // We can now insert with a shared reference!
        storage.insert(Cell::new(
            0,
            0,
            CellState::Alive,
            guard.current_state_mask(),
        ));
        storage.insert(Cell::new(
            1000000,
            -500000,
            CellState::Dead,
            guard.current_state_mask(),
        ));

        assert!(storage.find_and_apply(0, 0, |_| ()).is_some());
        assert!(storage.find_and_apply(1000000, -500000, |_| ()).is_some());
        assert!(storage.find_and_apply(1, 1, |_| ()).is_none());
    }

    #[test]
    fn test_space_level_synchronization() {
        let space = SimulationSpace::new(crate::BUCKET_COUNT);

        {
            let guard = space.mask.read();
            let current = guard.current_state_mask();
            let _next = guard.next_state_mask();

            // Add a cell
            let cell = Cell::new(10, 10, CellState::Dead, guard.current_state_mask());
            space.storage().insert(cell);

            space.storage().find_and_apply(10, 10, |c| {
                c.increment_neighbor_count(current);
                c.increment_neighbor_count(current);
                c.increment_neighbor_count(current);
                // Note: We'd need mutation for calculate_next_state if it's not internal.
                // But Cell::calculate_next_state takes &mut self.
                // However, in Phase 2, we will have exclusive access to nodes or use interior mutability.
            });
        }

        space.mask.cycle();
    }

    #[test]
    fn test_breeder_cell_count() {
        let space = SimulationSpace::new(crate::BUCKET_COUNT);
        let rle = include_str!("patterns/breeder1.rle");
        space.seed_from_rle(0, 0, rle);
        let mut cells = Vec::new();
        let guard = space.mask.read();
        space.storage().collect_all(
            guard.current_state_mask(),
            guard.last_state_mask(),
            guard.next_state_mask(),
            &mut cells,
        );
        // Breeder 1 is expected to have exactly 4060 cells.
        assert_eq!(
            cells.len(),
            4060,
            "Breeder 1 should have 4060 cells, found {}",
            cells.len()
        );
    }

    #[test]
    fn test_space_repair() {
        let space = SimulationSpace::new(crate::BUCKET_COUNT);
        let guard = space.mask.read();
        let current = guard.current_state_mask();

        space
            .storage()
            .insert(Cell::new(0, 0, CellState::Dead, current));

        // Manually corrupt neighbor counts
        space.storage().find_and_apply(0, 0, |c| {
            c.increment_neighbor_count(current);
            c.increment_neighbor_count(current);
        });

        // Verify corruption
        let mut count = 0;
        space.storage().find_and_apply(0, 0, |c| {
            count = c.get_neighbor_count(current);
        });
        assert_eq!(count, 2);

        // Run repair
        space.repair();

        // Verify fix - Any mask should now return 0
        space.storage().find_and_apply(0, 0, |c| {
            count = c.get_neighbor_count(current);
        });
        assert_eq!(count, 0);
    }

    #[test]
    fn test_rle_position_offset() {
        let space = SimulationSpace::new(crate::BUCKET_COUNT);
        // Test strict #R format (space separated)
        let rle = "#R 5 5\no!";
        space.seed_from_rle(0, 0, rle);

        let guard = space.mask.read();
        let _current = guard.current_state_mask();

        let mut found = false;
        space.storage().find_and_apply(5, 5, |_| found = true);
        assert!(found, "Cell should be at 5,5 due to offset");

        let mut found_origin = false;
        space
            .storage()
            .find_and_apply(0, 0, |_| found_origin = true);
        assert!(!found_origin, "Cell should NOT be at 0,0");
    }
}
