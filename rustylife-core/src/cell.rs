use crate::state::IndexGuard;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq)]
pub enum CellState {
    Alive,
    Dead,
}

#[derive(Debug)]
pub struct Cell {
    coords: (i128, i128),
    states: [CellState; 2],
    /// Bits 0-31: neighbor_count, Bits 32-63: last_index
    packed_neighbor_data: AtomicU64,
}

impl PartialEq for Cell {
    fn eq(&self, other: &Self) -> bool {
        self.coords == other.coords
            && self.states == other.states
            && self.packed_neighbor_data.load(Ordering::Relaxed)
                == other.packed_neighbor_data.load(Ordering::Relaxed)
    }
}

impl Cell {
    pub fn new(x: i128, y: i128, state: CellState, guard: &IndexGuard) -> Self {
        let last_index = guard.current() as u64;
        let packed = last_index << 32;
        Self {
            coords: (x, y),
            states: [state, state],
            packed_neighbor_data: AtomicU64::new(packed),
        }
    }

    pub fn coordinates(&self) -> (i128, i128) {
        self.coords
    }

    pub fn state(&self, idx: usize) -> CellState {
        self.states[idx]
    }

    pub fn set_state_at(&mut self, idx: usize, state: CellState) {
        self.states[idx] = state;
    }

    /// Increments the neighbor count using an atomic CAS loop to handle lazy reset.
    pub fn increment_neighbor_count(&self, current_index: usize) {
        let current_index = current_index as u64;
        let mut current_packed = self.packed_neighbor_data.load(Ordering::Acquire);

        loop {
            let last_index = current_packed >> 32;
            let new_packed = if last_index != current_index {
                // Lazy reset to 1
                (current_index << 32) | 1
            } else {
                // Increment current count
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

    /// Returns the current neighbor count. Performs lazy reset if index mismatch.
    pub fn get_neighbor_count(&self, current_index: usize) -> usize {
        let current_index = current_index as u64;
        let mut current_packed = self.packed_neighbor_data.load(Ordering::Acquire);

        loop {
            let last_index = current_packed >> 32;
            if last_index != current_index {
                // Lazy reset to 0
                let new_packed = current_index << 32;
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
                return (current_packed & 0xFFFFFFFF) as usize;
            }
        }
    }

    pub fn calculate_next_state(&mut self, current_idx: usize, next_idx: usize) {
        let count = self.get_neighbor_count(current_idx);
        if count == 3 {
            self.set_state_at(next_idx, CellState::Alive);
        } else if count == 2 {
            let current = self.state(current_idx);
            self.set_state_at(next_idx, current);
        } else {
            self.set_state_at(next_idx, CellState::Dead);
        }
    }

    pub fn is_permanently_dead(&self) -> bool {
        self.states[0] == CellState::Dead && self.states[1] == CellState::Dead
    }
}

// Manual implementation of Serialize/Deserialize to handle AtomicU64
impl Serialize for Cell {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let packed = self.packed_neighbor_data.load(Ordering::Relaxed);
        let mut state = serializer.serialize_struct("Cell", 3)?;
        state.serialize_field("coords", &self.coords)?;
        state.serialize_field("states", &self.states)?;
        state.serialize_field("packed_neighbor_data", &packed)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Cell {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct CellData {
            coords: (i128, i128),
            states: [CellState; 2],
            packed_neighbor_data: u64,
        }

        let data = CellData::deserialize(deserializer)?;
        Ok(Self {
            coords: data.coords,
            states: data.states,
            packed_neighbor_data: AtomicU64::new(data.packed_neighbor_data),
        })
    }
}

// Cell can no longer be Clone because of AtomicU64, but we can implement it manually if needed.
impl Clone for Cell {
    fn clone(&self) -> Self {
        Self {
            coords: self.coords,
            states: self.states,
            packed_neighbor_data: AtomicU64::new(self.packed_neighbor_data.load(Ordering::Relaxed)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SimulationIndex;

    #[test]
    fn test_cell_coordinates() {
        let manager = SimulationIndex::new();
        let guard = manager.read();
        let cell = Cell::new(10, -20, CellState::Dead, &guard);
        assert_eq!(cell.coordinates(), (10, -20));
    }

    #[test]
    fn test_cell_state_access() {
        let manager = SimulationIndex::new();
        let guard = manager.read();
        let mut cell = Cell::new(0, 0, CellState::Dead, &guard);
        assert_eq!(cell.state(0), CellState::Dead);

        cell.set_state_at(1, CellState::Alive);
        assert_eq!(cell.state(0), CellState::Dead);
        assert_eq!(cell.state(1), CellState::Alive);
    }

    #[test]
    fn test_optimized_neighbor_count() {
        let manager = SimulationIndex::new();
        let cell = {
            let guard = manager.read();
            Cell::new(0, 0, CellState::Alive, &guard)
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
        let manager = SimulationIndex::new();
        let guard = manager.read();
        let current = guard.current();
        let next = guard.next();

        // Rule: 3 neighbors -> Alive
        let mut cell = Cell::new(0, 0, CellState::Dead, &guard);
        cell.increment_neighbor_count(current);
        cell.increment_neighbor_count(current);
        cell.increment_neighbor_count(current);
        cell.calculate_next_state(current, next);
        assert_eq!(cell.state(next), CellState::Alive);

        // Rule: 2 neighbors -> Preserves (Alive stays Alive)
        let mut cell = Cell::new(0, 0, CellState::Alive, &guard);
        cell.increment_neighbor_count(current);
        cell.increment_neighbor_count(current);
        cell.calculate_next_state(current, next);
        assert_eq!(cell.state(next), CellState::Alive);

        // Rule: 2 neighbors -> Preserves (Dead stays Dead)
        let mut cell = Cell::new(0, 0, CellState::Dead, &guard);
        cell.increment_neighbor_count(current);
        cell.increment_neighbor_count(current);
        cell.calculate_next_state(current, next);
        assert_eq!(cell.state(next), CellState::Dead);

        // Rule: Other -> Dead (Underpopulation)
        let mut cell = Cell::new(0, 0, CellState::Alive, &guard);
        cell.increment_neighbor_count(current);
        cell.calculate_next_state(current, next);
        assert_eq!(cell.state(next), CellState::Dead);
    }
}
