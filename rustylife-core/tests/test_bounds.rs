use rustylife_core::block_tree::BlockTree;

#[test]
fn test_block_tree_bounds() {
    let mut tree = BlockTree::new();

    // 1. Empty tree -> None
    assert!(tree.bounds(1).is_none());

    // 2. Single cell at (0, 0)
    tree.set_cell(0, 0, 1, rustylife_core::cell::CellState::Alive);
    assert_eq!(tree.bounds(1), Some(((0, 0), (0, 0))));

    // 3. Add cell at (10, 10)
    tree.set_cell(10, 10, 1, rustylife_core::cell::CellState::Alive);
    assert_eq!(tree.bounds(1), Some(((0, 0), (10, 10))));

    // 4. Add cell at (-5, -5)
    tree.set_cell(-5, -5, 1, rustylife_core::cell::CellState::Alive);
    assert_eq!(tree.bounds(1), Some(((-5, -5), (10, 10))));

    // 5. Clear (0,0)
    tree.set_cell(0, 0, 1, rustylife_core::cell::CellState::Dead);
    assert_eq!(tree.bounds(1), Some(((-5, -5), (10, 10))));

    // 6. Clear (-5, -5)
    tree.set_cell(-5, -5, 1, rustylife_core::cell::CellState::Dead);
    assert_eq!(tree.bounds(1), Some(((10, 10), (10, 10))));
}
