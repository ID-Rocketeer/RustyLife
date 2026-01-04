use std::sync::{RwLock, RwLockReadGuard};

pub struct SimulationIndex {
    index: RwLock<usize>,
}

impl SimulationIndex {
    pub fn new() -> Self {
        Self {
            index: RwLock::new(0),
        }
    }

    /// Provides a read-only guard to the current state index.
    /// While this guard is held, the index cannot be flipped.
    pub fn read(&self) -> IndexGuard<'_> {
        IndexGuard {
            guard: self.index.read().expect("Lock poisoned"),
        }
    }

    /// Flips the global index (0 to 1 or 1 to 0).
    /// This requires a write lock, so it will block until all read guards are released.
    pub fn flip(&self) {
        let mut idx = self.index.write().expect("Lock poisoned");
        *idx = 1 - *idx;
    }

    /// For testing: tries to flip and returns false if it would block.
    #[cfg(test)]
    pub fn try_flip(&self) -> bool {
        if let Ok(mut idx) = self.index.try_write() {
            *idx = 1 - *idx;
            true
        } else {
            false
        }
    }
}

pub struct IndexGuard<'a> {
    guard: RwLockReadGuard<'a, usize>,
}

impl<'a> IndexGuard<'a> {
    pub fn current(&self) -> usize {
        *self.guard
    }

    pub fn next(&self) -> usize {
        1 - *self.guard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_toggles() {
        let manager = SimulationIndex::new();
        {
            let guard = manager.read();
            assert_eq!(guard.current(), 0);
            assert_eq!(guard.next(), 1);
        }

        manager.flip();

        {
            let guard = manager.read();
            assert_eq!(guard.current(), 1);
            assert_eq!(guard.next(), 0);
        }
    }

    #[test]
    fn test_flip_blocks_during_read() {
        let manager = SimulationIndex::new();
        let _guard = manager.read();

        // try_flip should fail because _guard is still in scope
        assert!(!manager.try_flip());
    }

    #[test]
    fn test_multi_reader() {
        let manager = SimulationIndex::new();
        {
            let _guard1 = manager.read();
            {
                let _guard2 = manager.read();
                {
                    let _guard3 = manager.read();
                    assert_eq!(_guard1.current(), 0);
                    assert_eq!(_guard2.current(), 0);
                    assert_eq!(_guard3.current(), 0);
                }
            }
        }
    }

    #[test]
    fn test_flip_blocks_until_all_readers_drop() {
        let manager = SimulationIndex::new();

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
