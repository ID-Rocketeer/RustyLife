// Copyright (C) 2026 Steven P. Collins. All rights reserved.
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use std::sync::{RwLock, RwLockReadGuard};

pub struct SimulationMasks {
    mask_lock: RwLock<usize>,
}

impl SimulationMasks {
    pub fn new() -> Self {
        Self {
            mask_lock: RwLock::new(0b0001),
        }
    }
}

impl Default for SimulationMasks {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulationMasks {
    /// Provides a read-only guard to the current state mask.
    /// While this guard is held, the mask cannot be cycled.
    pub fn read(&self) -> MaskGuard<'_> {
        MaskGuard {
            guard: self.mask_lock.read().expect("Lock poisoned"),
        }
    }

    /// Cycles the global mask (`0b0001 -> 0b0010 -> 0b0100 -> 0b1000`).
    /// This requires a write lock, so it will block until all read guards are released.
    pub fn cycle(&self) {
        let mut mask = self.mask_lock.write().expect("Lock poisoned");
        *mask = if *mask == 0b1000 { 0b0001 } else { *mask << 1 };
    }

    /// Resets the mask to the initial state (0b0001).
    pub fn reset(&self) {
        let mut mask = self.mask_lock.write().expect("Lock poisoned");
        *mask = 0b0001;
    }

    /// For testing: tries to cycle and returns false if it would block.
    #[cfg(test)]
    pub fn try_cycle(&self) -> bool {
        if let Ok(mut mask) = self.mask_lock.try_write() {
            *mask = if *mask == 0b1000 { 0b0001 } else { *mask << 1 };
            true
        } else {
            false
        }
    }
}

pub struct MaskGuard<'a> {
    guard: RwLockReadGuard<'a, usize>,
}

impl<'a> MaskGuard<'a> {
    pub fn current_state_mask(&self) -> usize {
        *self.guard
    }

    pub fn next_state_mask(&self) -> usize {
        if *self.guard == 0b1000 {
            0b0001
        } else {
            *self.guard << 1
        }
    }

    pub fn last_state_mask(&self) -> usize {
        if *self.guard == 0b0001 {
            0b1000
        } else {
            *self.guard >> 1
        }
    }

    pub fn last_last_state_mask(&self) -> usize {
        match *self.guard {
            0b0001 => 0b0100,
            0b0010 => 0b1000,
            other => other >> 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_advances() {
        let manager = SimulationMasks::new();
        {
            let guard = manager.read();
            assert_eq!(guard.current_state_mask(), 0b0001);
            assert_eq!(guard.next_state_mask(), 0b0010);
            assert_eq!(guard.last_state_mask(), 0b1000);
            assert_eq!(guard.last_last_state_mask(), 0b0100);
        }

        manager.cycle();

        {
            let guard = manager.read();
            assert_eq!(guard.current_state_mask(), 0b0010);
            assert_eq!(guard.next_state_mask(), 0b0100);
            assert_eq!(guard.last_state_mask(), 0b0001);
            assert_eq!(guard.last_last_state_mask(), 0b1000);
        }

        manager.cycle();

        {
            let guard = manager.read();
            assert_eq!(guard.current_state_mask(), 0b0100);
            assert_eq!(guard.next_state_mask(), 0b1000);
            assert_eq!(guard.last_state_mask(), 0b0010);
            assert_eq!(guard.last_last_state_mask(), 0b0001);
        }

        manager.cycle();

        {
            let guard = manager.read();
            assert_eq!(guard.current_state_mask(), 0b1000);
            assert_eq!(guard.next_state_mask(), 0b0001);
            assert_eq!(guard.last_state_mask(), 0b0100);
            assert_eq!(guard.last_last_state_mask(), 0b0010);
        }
    }

    #[test]
    fn test_cycle_blocks_during_read() {
        let manager = SimulationMasks::new();
        let _guard = manager.read();

        // try_cycle should fail because _guard is still in scope
        assert!(!manager.try_cycle());
    }

    #[test]
    fn test_multi_reader() {
        let manager = SimulationMasks::new();
        {
            let _guard1 = manager.read();
            {
                let _guard2 = manager.read();
                {
                    let _guard3 = manager.read();
                    assert_eq!(_guard1.current_state_mask(), 0b0001);
                    assert_eq!(_guard2.current_state_mask(), 0b0001);
                    assert_eq!(_guard3.current_state_mask(), 0b0001);
                }
            }
        }
    }

    #[test]
    fn test_cycle_blocks_until_all_readers_drop() {
        let manager = SimulationMasks::new();

        {
            let _guard1 = manager.read();
            {
                let _guard2 = manager.read();
                // try_cycle fails while both readers exist
                assert!(!manager.try_cycle());
            }
            // try_cycle still fails because _guard1 is still active
            assert!(!manager.try_cycle());
        }

        // Now that both guards are out of scope, it should succeed
        assert!(manager.try_cycle());
    }
}
