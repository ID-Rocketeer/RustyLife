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
