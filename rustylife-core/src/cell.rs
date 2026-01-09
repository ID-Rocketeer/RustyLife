use crate::state::MaskGuard;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq)]
pub enum CellState {
    Alive,
    Dead,
}

#[derive(Debug)]
pub struct Cell {
    coords: (i128, i128),
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
    pub fn new(x: i128, y: i128, state: CellState, guard: &MaskGuard) -> Self {
        let mask = guard.current_state_mask() as u8;
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

    /// Returns a 2-bit view for presenters:
    /// 0b11: Stable Alive (Alive in current, Alive in last)
    /// 0b10: New Born (Dead in last, Alive in current)
    /// 0b01: Dying (Alive in last, now dead in current)
    /// 0b00: Stable Dead (Erasure)
    pub fn presenter_view(&self, current_mask: usize, last_mask: usize) -> u8 {
        let current_bits = self.states.load(Ordering::Acquire);
        let current_alive = (current_bits & (current_mask as u8)) != 0;
        let last_alive = (current_bits & (last_mask as u8)) != 0;

        let mut view = 0u8;
        if current_alive {
            view |= 0b10;
        }
        if last_alive {
            view |= 0b01;
        }
        view
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

    /// Returns the current neighbor count. Performs lazy reset if index mismatch.
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

    pub fn calculate_next_state(&self, current_idx: usize, next_idx: usize) {
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
        self.states.load(Ordering::Acquire) == 0
    }
}

// Manual implementation of Serialize/Deserialize to handle AtomicU64
impl Serialize for Cell {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let packed_neighbors = self.packed_neighbor_data.load(Ordering::Relaxed);
        let states_raw = self.states.load(Ordering::Relaxed);
        let mut state = serializer.serialize_struct("Cell", 3)?;
        state.serialize_field("coords", &self.coords)?;
        state.serialize_field("states", &states_raw)?;
        state.serialize_field("packed_neighbor_data", &packed_neighbors)?;
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
            states: u8,
            packed_neighbor_data: u8,
        }

        let data = CellData::deserialize(deserializer)?;
        Ok(Self {
            coords: data.coords,
            states: AtomicU8::new(data.states),
            packed_neighbor_data: AtomicU8::new(data.packed_neighbor_data),
        })
    }
}

// Cell can no longer be Clone because of AtomicU64, but we can implement it manually if needed.
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
        let cell = Cell::new(10, -20, CellState::Dead, &guard);
        assert_eq!(cell.coordinates(), (10, -20));
    }

    #[test]
    fn test_cell_state_access() {
        let manager = SimulationMasks::new();
        let guard = manager.read();
        let cell = Cell::new(0, 0, CellState::Dead, &guard);
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
        let manager = SimulationMasks::new();
        let guard = manager.read();
        let current = guard.current_state_mask();
        let next = guard.next_state_mask();

        // Rule: 3 neighbors -> Alive
        let cell = Cell::new(0, 0, CellState::Dead, &guard);
        cell.increment_neighbor_count(current);
        cell.increment_neighbor_count(current);
        cell.increment_neighbor_count(current);
        cell.calculate_next_state(current, next);
        assert_eq!(cell.state(next), CellState::Alive);

        // Rule: 2 neighbors -> Preserves (Alive stays Alive)
        let cell = Cell::new(0, 0, CellState::Alive, &guard);
        cell.increment_neighbor_count(current);
        cell.increment_neighbor_count(current);
        cell.calculate_next_state(current, next);
        assert_eq!(cell.state(next), CellState::Alive);

        // Rule: 2 neighbors -> Preserves (Dead stays Dead)
        let cell = Cell::new(0, 0, CellState::Dead, &guard);
        cell.increment_neighbor_count(current);
        cell.increment_neighbor_count(current);
        cell.calculate_next_state(current, next);
        assert_eq!(cell.state(next), CellState::Dead);

        // Rule: Other -> Dead (Underpopulation)
        let cell = Cell::new(0, 0, CellState::Alive, &guard);
        cell.increment_neighbor_count(current);
        cell.calculate_next_state(current, next);
        assert_eq!(cell.state(next), CellState::Dead);
    }
}
