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

    // Mark the child node as Alive so it should be preserved
    tree.arena.nodes[child_idx as usize].block.boards[0] = 1;

    assert!(tree.arena.nodes[root_idx as usize].block.is_dead());
    assert!(!tree.arena.nodes[child_idx as usize].block.is_dead());
    assert!(tree.arena.nodes[grandchild_idx as usize].block.is_dead());

    // Initial Count: 3
    assert_eq!(tree.arena.nodes.len(), 3);

    // Prune: Should rebuild the tree with ONLY the alive blocks.
    // The dead root and dead grandchild should be removed in one pass.
    tree.prune();

    assert_eq!(
        tree.arena.nodes.len(),
        1,
        "Pruning should remove dead blocks instantly, keeping only the 1 alive block"
    );

    // The remaining node should have the coordinates of the survivor (10, 10)
    assert_eq!(tree.arena.nodes[0].bx, 10);
    assert_eq!(tree.arena.nodes[0].by, 10);
}
