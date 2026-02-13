#[cfg(test)]
mod tests {
    use rustylife_core::block_tree::{Block8x8, BlockTree};
    use rustylife_core::cell::CellState;

    #[test]
    fn test_block8x8_basic() {
        let mut block = Block8x8::new();
        // Mask 1 -> Index 0
        block.set_bit(0, 0, 0, true);
        block.set_bit(7, 7, 0, true);

        assert!(block.get_bit(0, 0, 0));
        assert!(block.get_bit(7, 7, 0));
        assert!(!block.get_bit(0, 1, 0));

        // Mask 2 -> Index 1 (Should be empty initially)
        assert!(!block.get_bit(0, 0, 1));

        // Set bit in Mask 2
        block.set_bit(0, 0, 1, true);
        assert!(block.get_bit(0, 0, 1));
    }

    #[test]
    fn test_block_tree_integration() {
        let mut tree = BlockTree::new();
        let mask1 = 1; // 001

        // Origin
        tree.set_cell(0, 0, mask1, CellState::Alive);
        assert_eq!(tree.get_cell(0, 0, mask1), CellState::Alive);
        assert_eq!(tree.get_cell(1, 0, mask1), CellState::Dead);

        // Far away (Different block)
        tree.set_cell(100, 100, mask1, CellState::Alive);
        assert_eq!(tree.get_cell(100, 100, mask1), CellState::Alive);
        assert_eq!(tree.get_cell(101, 100, mask1), CellState::Dead);

        // Negative coordinates
        tree.set_cell(-5, -5, mask1, CellState::Alive);
        assert_eq!(tree.get_cell(-5, -5, mask1), CellState::Alive);
        // Check block mapping for negative
        // -5 >> 3 = -1. (-1 * 8) = -8. -5 is offset 3 from -8?
        // x & 7 logic: -5 = ...11111011. & 7 = 011 = 3.
        // So local x = 3. Correct.
    }

    #[test]
    fn test_simd_step() {
        let mut block = Block8x8::new();
        // Blinker pattern (Vertical line of 3)
        // Center: (1, 1), (1, 2), (1, 3)
        // Note: Block local coords are 0-7.

        // Setup in Mask 1 (Index 0)
        block.set_bit(1, 1, 0, true);
        block.set_bit(1, 2, 0, true);
        block.set_bit(1, 3, 0, true);

        // Step from 0 -> 1
        // Neighbors are 0.
        let (pop, born, died, work, is_dead) = block.step(0, 0, 0, 0, 0, 0, 0, 0, 0, 1);

        // Verify Metrics
        // New state has 3 cells (horizontal). Pop should be 3.
        assert_eq!(pop, 3, "Population should be 3");

        // Born: (0,2) and (2,2). Count = 2.
        assert_eq!(born, 2, "Born count should be 2");

        // Died: (1,1) and (1,3). Count = 2.
        assert_eq!(died, 2, "Died count should be 2");

        // Work = Union. Old (3) | New (3). Total 5 distinct pixels?
        // Work = Union. Old (3) | New (3). Total 5 distinct pixels?
        // Vertical: (1,1), (1,2), (1,3).
        // Horizontal: (0,2), (1,2), (2,2).
        // Union: (1,1), (1,2), (1,3), (0,2), (2,2). 5 pixels.
        // Wait, (1,2) overlaps.
        assert_eq!(work, 5, "Work should be 5 (Union of inputs and outputs)");
        assert!(!is_dead);

        // Expect Horizontal line in Mask 2 (Index 1)
        // (0, 2), (1, 2), (2, 2)
        assert!(block.get_bit(0, 2, 1));
        assert!(block.get_bit(1, 2, 1));
        assert!(block.get_bit(2, 2, 1));

        // (1, 1) and (1, 3) should die
        assert!(!block.get_bit(1, 1, 1));
        assert!(!block.get_bit(1, 3, 1));
    }

    #[test]
    fn test_allocation_growth() {
        use rustylife_core::block_tree::BlockArena;
        let mut arena = BlockArena::new();
        // Initial capacity is 1024
        assert_eq!(arena.nodes.capacity(), 1024);

        // Fill it up
        for _ in 0..1024 {
            arena.alloc(0, 0);
        }
        assert_eq!(arena.nodes.len(), 1024);
        assert_eq!(arena.nodes.capacity(), 1024);

        // Trigger one more alloc - should grow by CHUNK (1024), not Double (2048)
        arena.alloc(0, 0);
        assert_eq!(arena.nodes.len(), 1025);
        assert_eq!(arena.nodes.capacity(), 2048); // 1024 + 1024
    }
}
