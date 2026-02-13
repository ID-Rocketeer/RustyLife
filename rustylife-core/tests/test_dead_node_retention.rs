use rustylife_core::block_tree::BlockTree;

#[test]
fn test_dead_node_retention() {
    let mut tree = BlockTree::new();

    // Create a Parent -> Child relationship
    // Parent: (0, 0) - Dead
    // Child: (10, 10) - Alive

    let parent_idx = tree.arena.alloc(0, 0);
    tree.root = Some(parent_idx);

    let child_idx = tree.arena.alloc(10, 10);
    tree.arena.nodes[parent_idx as usize].right = Some(child_idx);

    // Mark Parent as dead
    assert!(tree.arena.nodes[parent_idx as usize].block.is_dead());

    // Mark Child as ALIVE (manually set a bit)
    tree.arena.nodes[child_idx as usize].block.boards[0] = 1;
    assert!(!tree.arena.nodes[child_idx as usize].block.is_dead());

    // Prune 1: Parent is Dead. Aggressive Pruning REBUILDS the tree.
    // Parent should be removed. Child should be re-inserted.
    tree.prune();

    assert_eq!(
        tree.arena.nodes.len(),
        1,
        "Parent should be aggressively pruned, Child retained"
    );
    assert!(
        tree.arena.nodes.iter().any(|n| n.bx == 10 && n.by == 10),
        "Child (10,10) exists"
    );
    assert!(
        !tree.arena.nodes.iter().any(|n| n.bx == 0 && n.by == 0),
        "Parent (0,0) gone"
    );

    // Check Child is now a leaf (Parent is gone)
    let child = &tree.arena.nodes[0];
    assert!(child.left.is_none());
    assert!(child.right.is_none());

    // Now kill the Child
    // Note: Index 0 is the only node left (Child)
    tree.arena.nodes[0].block.boards[0] = 0; // Clear bit

    // Prune 2: Child is Dead Leaf -> Pruned.
    tree.prune();
    assert_eq!(tree.arena.nodes.len(), 0, "Child pruned (Tree empty)");
}
