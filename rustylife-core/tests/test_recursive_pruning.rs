use rustylife_core::block_tree::BlockTree;

#[test]
fn test_recursive_pruning() {
    let mut tree = BlockTree::new();

    // Create a chain of blocks: Root -> Child -> Grandchild
    // Use coordinates that will likely cause this structure (BST depends on insertion order and value)
    // Root: (0, 0)
    // Child: (10, 10) -> Greater than (0,0) -> Right child
    // Grandchild: (20, 20) -> Greater than (10,10) -> Right child

    let root_idx = tree.arena.alloc(0, 0);
    tree.root = Some(root_idx);

    let child_idx = tree.arena.alloc(10, 10);
    tree.arena.nodes[root_idx as usize].right = Some(child_idx);

    let grandchild_idx = tree.arena.alloc(20, 20);
    tree.arena.nodes[child_idx as usize].right = Some(grandchild_idx);

    // Mark all as dead
    // (Newly allocated blocks are empty/dead by default)
    assert!(tree.arena.nodes[root_idx as usize].block.is_dead());
    assert!(tree.arena.nodes[child_idx as usize].block.is_dead());
    assert!(tree.arena.nodes[grandchild_idx as usize].block.is_dead());

    // Initial Count: 3
    assert_eq!(tree.arena.nodes.len(), 3);

    // Prune 1: Should remove Grandchild (Dead Leaf). Child becomes Leaf.
    tree.prune();
    assert_eq!(
        tree.arena.nodes.len(),
        2,
        "Pass 1: Should remove grandchild"
    );

    // Prune 2: Should remove Child (Now Dead Leaf). Root becomes Leaf.
    tree.prune();
    assert_eq!(tree.arena.nodes.len(), 1, "Pass 2: Should remove child");

    // Prune 3: Should remove Root (Now Dead Leaf). Tree empty.
    tree.prune();
    assert_eq!(tree.arena.nodes.len(), 0, "Pass 3: Should remove root");
    assert!(tree.root.is_none());
}
