use crate::cell::Cell;
use crate::hash::hash_coordinates;
use crate::state::{IndexGuard, SimulationIndex};
use crate::tree::CellTree;

/// Sparse storage using 256 buckets, each holding a BST of Cells.
pub struct SparseStorage {
    pub buckets: [CellTree; 256],
}

impl SparseStorage {
    pub fn new() -> Self {
        let buckets = std::array::from_fn(|_| CellTree::new());
        Self { buckets }
    }

    pub fn insert(&self, cell: Cell) {
        let coords = cell.coordinates();
        let idx = hash_coordinates(coords.0, coords.1);
        self.buckets[idx].insert(cell);
    }

    pub fn find_and_apply<F, R>(&self, x: i128, y: i128, f: F) -> Option<R>
    where
        F: FnOnce(&Cell) -> R,
    {
        let idx = hash_coordinates(x, y);
        self.buckets[idx].find_and_apply((x, y), f)
    }

    pub fn find_or_create_and_apply<F, C, R>(&self, x: i128, y: i128, creator: C, f: F) -> R
    where
        C: FnOnce() -> Cell,
        F: FnOnce(&Cell) -> R,
    {
        let idx = hash_coordinates(x, y);
        self.buckets[idx].find_or_create_and_apply((x, y), creator, f)
    }

    pub fn collect_living(&self, current_idx: usize, out: &mut Vec<(i128, i128)>) {
        for bucket in &self.buckets {
            bucket.collect_living(current_idx, out);
        }
    }
}

/// An arbitrary space that holds sparse cell data and is governed by a global simulation index.
pub struct SimulationSpace {
    pub index: SimulationIndex,
    pub storage: SparseStorage,
}

impl SimulationSpace {
    pub fn new() -> Self {
        Self {
            index: SimulationIndex::new(),
            storage: SparseStorage::new(),
        }
    }

    /// Aquires a shared read lock for the space.
    pub fn read(&self) -> IndexGuard<'_> {
        self.index.read()
    }

    /// Acquires an exclusive write lock to flip the space's global index.
    pub fn flip(&self) {
        self.index.flip()
    }

    pub fn collect_living(&self) -> Vec<(i128, i128)> {
        let guard = self.read();
        let current_idx = guard.current();
        let mut living = Vec::new();
        self.storage.collect_living(current_idx, &mut living);
        living
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::{Cell, CellState};

    #[test]
    fn test_sparse_storage_parallel_traits() {
        let storage = SparseStorage::new();
        let manager = SimulationIndex::new();
        let guard = manager.read();

        // We can now insert with a shared reference!
        storage.insert(Cell::new(0, 0, CellState::Alive, &guard));
        storage.insert(Cell::new(1000000, -500000, CellState::Dead, &guard));

        assert!(storage.find_and_apply(0, 0, |_| ()).is_some());
        assert!(storage.find_and_apply(1000000, -500000, |_| ()).is_some());
        assert!(storage.find_and_apply(1, 1, |_| ()).is_none());
    }

    #[test]
    fn test_space_level_synchronization() {
        let space = SimulationSpace::new();

        {
            let guard = space.index.read();
            let current = guard.current();
            let _next = guard.next();

            // Add a cell
            let cell = Cell::new(10, 10, CellState::Dead, &guard);
            space.storage.insert(cell);

            space.storage.find_and_apply(10, 10, |c| {
                c.increment_neighbor_count(current);
                c.increment_neighbor_count(current);
                c.increment_neighbor_count(current);
                // Note: We'd need mutation for calculate_next_state if it's not internal.
                // But Cell::calculate_next_state takes &mut self.
                // However, in Phase 2, we will have exclusive access to nodes or use interior mutability.
            });
        }

        space.index.flip();
    }
}
