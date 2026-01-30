use std::sync::atomic::{AtomicU8, Ordering};

/// Represents the possible states of a single cell in the simulation.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum CellState {
    /// The cell is currently alive.
    Alive,
    /// The cell is currently dead.
    Dead,
}

/// Represents a single cell in the simulation using a multi-generational state tracking system.
///
/// Each cell tracks its state across three generations using bitmasks, allowing for
/// lock-free, concurrent updates without double-buffering the entire storage.
#[derive(Debug)]
pub struct Cell {
    /// Lexicographical coordinates (x, y).
    coords: (i128, i128),
    /// Bitmask of states across different generation masks.
    states: AtomicU8,
    /// Bits 0-3: neighbor_count, Bits 4-7: last_mask
    packed_neighbor_data: AtomicU8,
}

impl PartialEq for Cell {
    fn eq(&self, other: &Self) -> bool {
        self.coords == other.coords
            && self.states.load(Ordering::Relaxed) == other.states.load(Ordering::Relaxed)
            && self.packed_neighbor_data.load(Ordering::Relaxed)
                == other.packed_neighbor_data.load(Ordering::Relaxed)
    }
}

impl Cell {
    pub fn new(x: i128, y: i128, state: CellState, current_mask: usize) -> Self {
        let mask = current_mask as u8;
        let packed = mask << 4; // count is 0
        let bits = if state == CellState::Alive { mask } else { 0 };
        Self {
            coords: (x, y),
            states: AtomicU8::new(bits),
            packed_neighbor_data: AtomicU8::new(packed),
        }
    }

    pub fn coordinates(&self) -> (i128, i128) {
        self.coords
    }

    pub fn state(&self, mask: usize) -> CellState {
        if (self.states.load(Ordering::Acquire) & (mask as u8)) != 0 {
            CellState::Alive
        } else {
            CellState::Dead
        }
    }

    pub fn set_state_at(&self, mask: usize, state: CellState) {
        match state {
            CellState::Alive => {
                self.states.fetch_or(mask as u8, Ordering::Release);
            }
            CellState::Dead => {
                self.states.fetch_and(!(mask as u8), Ordering::Release);
            }
        }
    }

    /// Returns a view for presenters supporting 4-state lifecycle tracking.
    /// Returns `Some(state)` for visible cells, `None` for stable dead (erased) cells.
    ///
    /// Visibility logic uses 3 generations of history (current, last, last_last):
    /// - `Some(0b11)` (3): Stable Alive (Alive in current, Alive in last)
    /// - `Some(0b10)` (2): New Born (Dead in last, Alive in current)
    /// - `Some(0b01)` (1): Dying (Alive in last, Dead in current)
    /// - `Some(0b00)` (0): Newly Dead / Erasure (Dead in current, Dead in last, but was Alive in last_last)
    ///
    /// Pruning Note: A cell is only pruned from the storage tree once it becomes Truly Dead (None),
    /// which happens only after it has been dead for 3 generations straight.
    pub fn presenter_view(
        &self,
        current_mask: usize,
        last_mask: usize,
        last_last_mask: usize,
    ) -> Option<u8> {
        let bits = self.states.load(Ordering::Acquire);
        let current_alive = (bits & (current_mask as u8)) != 0;
        let last_alive = (bits & (last_mask as u8)) != 0;

        if current_alive {
            if last_alive {
                Some(0b11) // Precise: Stable Alive
            } else {
                Some(0b10) // Precise: New Born
            }
        } else if last_alive {
            Some(0b01) // Precise: Dying
        } else if (bits & (last_last_mask as u8)) != 0 {
            Some(0b00) // Precise: Newly Dead / Erasure (Ghost frame)
        } else {
            None // Stable Dead
        }
    }

    /// Increments the neighbor count using an atomic CAS loop to handle lazy reset.
    pub fn increment_neighbor_count(&self, current_mask: usize) {
        let current_mask = current_mask as u8;
        let mut current_packed = self.packed_neighbor_data.load(Ordering::Acquire);

        loop {
            let last_mask = current_packed >> 4;
            let new_packed = if last_mask != current_mask {
                // Lazy reset to 1
                (current_mask << 4) | 1
            } else {
                // Increment current count (lower 4 bits)
                current_packed + 1
            };

            match self.packed_neighbor_data.compare_exchange_weak(
                current_packed,
                new_packed,
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(actual) => current_packed = actual,
            }
        }
    }

    /// Returns the current neighbor count. Performs lazy reset if mask mismatch.
    /// The misnatch is an indicator that no living neighbors existed to update the cell.
    pub fn get_neighbor_count(&self, current_mask: usize) -> usize {
        let current_mask = current_mask as u8;
        let mut current_packed = self.packed_neighbor_data.load(Ordering::Acquire);

        loop {
            let last_mask = current_packed >> 4;
            if last_mask != current_mask {
                // Lazy reset to 0
                let new_packed = current_mask << 4;
                match self.packed_neighbor_data.compare_exchange_weak(
                    current_packed,
                    new_packed,
                    Ordering::Release,
                    Ordering::Acquire,
                ) {
                    Ok(_) => return 0,
                    Err(actual) => current_packed = actual,
                }
            } else {
                return (current_packed & 0x0F) as usize;
            }
        }
    }

    /// Forcefully resets the neighbor count to 0 for the given mask.
    /// This is used during the "Repair Phase" to clear partial counts from an interrupted generation.
    pub fn reset_neighbor_count(&self, current_mask: usize) {
        let mask = current_mask as u8;
        let packed = mask << 4; // count is 0
        self.packed_neighbor_data.store(packed, Ordering::Release);
    }

    pub fn calculate_next_state(&self, current_mask: usize, next_mask: usize) -> CellState {
        let count = self.get_neighbor_count(current_mask);

        if count == 3 {
            self.set_state_at(next_mask, CellState::Alive);
            CellState::Alive
        } else if count == 2 {
            let current = self.state(current_mask);
            self.set_state_at(next_mask, current);
            current
        } else {
            self.set_state_at(next_mask, CellState::Dead);
            CellState::Dead
        }
    }

    pub fn is_permanently_dead(&self) -> bool {
        self.states.load(Ordering::Acquire) == 0
    }
}

// Cell can no longer be Clone because of AtomicU8, but we can implement it manually if needed.
impl Clone for Cell {
    fn clone(&self) -> Self {
        Self {
            coords: self.coords,
            states: AtomicU8::new(self.states.load(Ordering::Relaxed)),
            packed_neighbor_data: AtomicU8::new(self.packed_neighbor_data.load(Ordering::Relaxed)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SimulationMasks;

    #[test]
    fn test_cell_coordinates() {
        let manager = SimulationMasks::new();
        let guard = manager.read();
        let cell = Cell::new(10, -20, CellState::Dead, guard.current_state_mask());
        assert_eq!(cell.coordinates(), (10, -20));
    }

    #[test]
    fn test_cell_state_access() {
        let manager = SimulationMasks::new();
        let guard = manager.read();
        let cell = Cell::new(0, 0, CellState::Dead, guard.current_state_mask());
        assert_eq!(cell.state(0), CellState::Dead);

        cell.set_state_at(1, CellState::Alive);
        assert_eq!(cell.state(0), CellState::Dead);
        assert_eq!(cell.state(1), CellState::Alive);
    }

    #[test]
    fn test_optimized_neighbor_count() {
        let manager = SimulationMasks::new();
        let cell = {
            let guard = manager.read();
            Cell::new(0, 0, CellState::Alive, guard.current_state_mask())
        };

        // Same generation: increments normally
        cell.increment_neighbor_count(0);
        cell.increment_neighbor_count(0);
        assert_eq!(cell.get_neighbor_count(0), 2);

        // New generation: read resets to 0
        assert_eq!(cell.get_neighbor_count(1), 0);

        // New generation: increment resets to 1
        cell.increment_neighbor_count(2);
        assert_eq!(cell.get_neighbor_count(2), 1);
    }

    #[test]
    fn test_transition_rules_with_internal_count() {
        let manager = SimulationMasks::new();
        let guard = manager.read();
        let current = guard.current_state_mask();
        let next = guard.next_state_mask();

        // Rule: 3 neighbors -> Alive
        let cell = Cell::new(0, 0, CellState::Dead, current);
        cell.increment_neighbor_count(current);
        cell.increment_neighbor_count(current);
        cell.increment_neighbor_count(current);
        cell.calculate_next_state(current, next);
        assert_eq!(cell.state(next), CellState::Alive);

        // Rule: 2 neighbors -> Preserves (Alive stays Alive)
        let cell = Cell::new(0, 0, CellState::Alive, current);
        cell.increment_neighbor_count(current);
        cell.increment_neighbor_count(current);
        cell.calculate_next_state(current, next);
        assert_eq!(cell.state(next), CellState::Alive);

        // Rule: 2 neighbors -> Preserves (Dead stays Dead)
        let cell = Cell::new(0, 0, CellState::Dead, current);
        cell.increment_neighbor_count(current);
        cell.increment_neighbor_count(current);
        cell.calculate_next_state(current, next);
        assert_eq!(cell.state(next), CellState::Dead);

        // Rule: Other -> Dead (Underpopulation)
        let cell = Cell::new(0, 0, CellState::Alive, current);
        cell.increment_neighbor_count(current);
        cell.calculate_next_state(current, next);
        assert_eq!(cell.state(next), CellState::Dead);
    }

    #[test]
    fn test_reset_neighbor_count() {
        let manager = SimulationMasks::new();
        let guard = manager.read();
        let cell = Cell::new(0, 0, CellState::Alive, guard.current_state_mask());

        cell.increment_neighbor_count(guard.current_state_mask());
        cell.increment_neighbor_count(guard.current_state_mask());
        assert_eq!(cell.get_neighbor_count(guard.current_state_mask()), 2);

        cell.reset_neighbor_count(guard.current_state_mask());
        assert_eq!(cell.get_neighbor_count(guard.current_state_mask()), 0);
    }
}
