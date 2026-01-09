use std::sync::{RwLock, RwLockReadGuard};

pub struct SimulationMasks {
    mask_lock: RwLock<usize>,
}

impl SimulationMasks {
    pub fn new() -> Self {
        Self {
            mask_lock: RwLock::new(0b001),
        }
    }

    /// Provides a read-only guard to the current state index.
    /// While this guard is held, the index cannot be flipped.
    pub fn read(&self) -> MaskGuard<'_> {
        MaskGuard {
            guard: self.mask_lock.read().expect("Lock poisoned"),
        }
    }

    /// Flips the global index (0b001 -> 0b010 -> 0b100).
    /// This requires a write lock, so it will block until all read guards are released.
    pub fn flip(&self) {
        let mut idx = self.mask_lock.write().expect("Lock poisoned");
        *idx = if *idx == 0b100 { 0b001 } else { *idx << 1 };
    }

    /// For testing: tries to flip and returns false if it would block.
    #[cfg(test)]
    pub fn try_flip(&self) -> bool {
        if let Ok(mut idx) = self.mask_lock.try_write() {
            *idx = if *idx == 0b100 { 0b001 } else { *idx << 1 };
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
        if *self.guard == 0b100 {
            0b001
        } else {
            *self.guard << 1
        }
    }

    pub fn last_state_mask(&self) -> usize {
        if *self.guard == 0b001 {
            0b100
        } else {
            *self.guard >> 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_toggles() {
        let manager = SimulationMasks::new();
        {
            let guard = manager.read();
            assert_eq!(guard.current_state_mask(), 0b001);
            assert_eq!(guard.next_state_mask(), 0b010);
            assert_eq!(guard.last_state_mask(), 0b100);
        }

        manager.flip();

        {
            let guard = manager.read();
            assert_eq!(guard.current_state_mask(), 0b010);
            assert_eq!(guard.next_state_mask(), 0b100);
            assert_eq!(guard.last_state_mask(), 0b001);
        }

        manager.flip();

        {
            let guard = manager.read();
            assert_eq!(guard.current_state_mask(), 0b100);
            assert_eq!(guard.next_state_mask(), 0b001);
            assert_eq!(guard.last_state_mask(), 0b010);
        }
    }

    #[test]
    fn test_flip_blocks_during_read() {
        let manager = SimulationMasks::new();
        let _guard = manager.read();

        // try_flip should fail because _guard is still in scope
        assert!(!manager.try_flip());
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
                    assert_eq!(_guard1.current_state_mask(), 0b001);
                    assert_eq!(_guard2.current_state_mask(), 0b001);
                    assert_eq!(_guard3.current_state_mask(), 0b001);
                }
            }
        }
    }

    #[test]
    fn test_flip_blocks_until_all_readers_drop() {
        let manager = SimulationMasks::new();

        {
            let _guard1 = manager.read();
            {
                let _guard2 = manager.read();
                // try_flip fails while both readers exist
                assert!(!manager.try_flip());
            }
            // try_flip still fails because _guard1 is still active
            assert!(!manager.try_flip());
        }

        // Now that both guards are out of scope, it should succeed
        assert!(manager.try_flip());
    }
}
