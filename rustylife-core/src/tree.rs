use crate::cell::Cell;
use std::cmp::Ordering;
use std::io::Write;
use std::sync::RwLock;

pub struct CellNode {
    pub cell: Cell,
    pub left: Option<Box<CellNode>>,
    pub right: Option<Box<CellNode>>,
}

impl CellNode {
    pub fn new(cell: Cell) -> Self {
        Self {
            cell,
            left: None,
            right: None,
        }
    }

    pub fn delete(self: Box<Self>) -> Option<Box<Self>> {
        let left = self.left;
        let right = self.right;

        match (left, right) {
            (None, None) => None,
            (Some(l), None) => Some(l),
            (None, Some(r)) => Some(r),
            (Some(l), Some(r)) => {
                let (min_node, new_right) = Self::extract_min(r);
                let mut replacement = min_node;
                replacement.left = Some(l);
                replacement.right = new_right;
                Some(replacement)
            }
        }
    }

    fn extract_min(mut node: Box<Self>) -> (Box<Self>, Option<Box<Self>>) {
        if node.left.is_some() {
            let left_node = node.left.take().unwrap();
            let (min, replacement) = Self::extract_min(left_node);
            node.left = replacement;
            (min, Some(node))
        } else {
            let right = node.right.take();
            (node, right)
        }
    }
}

pub struct CellTree {
    pub root: Option<Box<CellNode>>,
}

impl CellTree {
    pub fn new() -> Self {
        Self { root: None }
    }

    pub fn insert(&mut self, cell: Cell) {
        let coords = cell.coordinates();
        self.find_or_create_and_apply(coords, || cell, |_| ());
    }

    pub fn clear(&mut self) {
        self.root = None;
    }

    pub fn find_and_apply<F, R>(&self, coords: (i128, i128), f: F) -> Option<R>
    where
        F: FnOnce(&Cell) -> R,
    {
        self.root
            .as_ref()
            .and_then(|node| Self::find_recursive(node, coords, f))
    }

    fn find_recursive<F, R>(node: &CellNode, coords: (i128, i128), f: F) -> Option<R>
    where
        F: FnOnce(&Cell) -> R,
    {
        match Self::compare_coords(coords, node.cell.coordinates()) {
            Ordering::Less => node
                .left
                .as_ref()
                .and_then(|n| Self::find_recursive(n, coords, f)),
            Ordering::Greater => node
                .right
                .as_ref()
                .and_then(|n| Self::find_recursive(n, coords, f)),
            Ordering::Equal => Some(f(&node.cell)),
        }
    }

    pub fn find_or_create_and_apply<F, C, R>(&mut self, coords: (i128, i128), creator: C, f: F) -> R
    where
        C: FnOnce() -> Cell,
        F: FnOnce(&mut Cell) -> R, // Changed to &mut Cell
    {
        // The initial root handling can be simplified by just calling the recursive function
        // which handles the None case for the root as well.
        Self::find_or_create_recursive(&mut self.root, coords, creator, f)
    }

    fn find_or_create_recursive<F, C, R>(
        node_opt: &mut Option<Box<CellNode>>,
        coords: (i128, i128),
        creator: C,
        f: F,
    ) -> R
    where
        C: FnOnce() -> Cell,
        F: FnOnce(&mut Cell) -> R,
    {
        if let Some(node) = node_opt {
            match Self::compare_coords(coords, node.cell.coordinates()) {
                Ordering::Equal => f(&mut node.cell),
                Ordering::Less => {
                    Self::find_or_create_recursive(&mut node.left, coords, creator, f)
                }
                Ordering::Greater => {
                    Self::find_or_create_recursive(&mut node.right, coords, creator, f)
                }
            }
        } else {
            let new_node = Box::new(CellNode::new(creator()));
            *node_opt = Some(new_node);
            f(&mut node_opt.as_mut().unwrap().cell)
        }
    }

    /// Applies updates to the tree in a batch using a sorted list of coordinates.
    /// This is an O(N + M) operation where N is tree size and M is number of updates.
    /// It effectively rebuilds the tree, creating new nodes where necessary.
    /// Applies updates to the tree in-place using a sorted list of coordinates.
    /// Does NOT rebuild the tree, only modifies existing nodes or inserts new ones where needed.
    pub fn apply_batch<C, F>(&mut self, sorted_coords: &[(i128, i128)], creator: C, applicator: F)
    where
        C: Fn(i128, i128) -> Cell,
        F: Fn(&mut Cell),
    {
        Self::apply_recursive(&mut self.root, sorted_coords, &creator, &applicator);
    }

    fn apply_recursive<C, F>(
        node_opt: &mut Option<Box<CellNode>>,
        coords: &[(i128, i128)],
        creator: &C,
        applicator: &F,
    ) where
        C: Fn(i128, i128) -> Cell,
        F: Fn(&mut Cell),
    {
        if coords.is_empty() {
            return;
        }

        if let Some(node) = node_opt {
            let n_coords = node.cell.coordinates();

            // Partition coords into: [ < n_coords ], [ == n_coords ], [ > n_coords ]
            let start_idx =
                coords.partition_point(|c| Self::compare_coords(*c, n_coords) == Ordering::Less);

            let mut end_idx = start_idx;
            while end_idx < coords.len() && coords[end_idx] == n_coords {
                end_idx += 1;
            }

            Self::apply_recursive(&mut node.left, &coords[..start_idx], creator, applicator);

            for _ in start_idx..end_idx {
                applicator(&mut node.cell);
            }

            Self::apply_recursive(&mut node.right, &coords[end_idx..], creator, applicator);
        } else {
            *node_opt = Self::build_from_coords(coords, creator, applicator);
        }
    }

    fn build_from_coords<C, F>(
        coords: &[(i128, i128)],
        creator: &C,
        applicator: &F,
    ) -> Option<Box<CellNode>>
    where
        C: Fn(i128, i128) -> Cell,
        F: Fn(&mut Cell),
    {
        if coords.is_empty() {
            return None;
        }

        let mid = coords.len() / 2;
        let mid_coord = coords[mid];

        // Scan for range of identical mid_coords
        let mut start = mid;
        while start > 0 && coords[start - 1] == mid_coord {
            start -= 1;
        }

        let mut end = mid + 1;
        while end < coords.len() && coords[end] == mid_coord {
            end += 1;
        }

        let mut new_node = Box::new(CellNode::new(creator(mid_coord.0, mid_coord.1)));

        for _ in start..end {
            applicator(&mut new_node.cell);
        }

        new_node.left = Self::build_from_coords(&coords[..start], creator, applicator);
        new_node.right = Self::build_from_coords(&coords[end..], creator, applicator);

        Some(new_node)
    }

    // This function is now redundant with find_or_create_recursive,
    // This function is now redundant with find_or_create_recursive,
    // and its logic was based on RwLock. Removing it as per instruction.
    /*
    fn find_or_create_recursive<F, C, R>(
        node: &CellNode,
        coords: (i128, i128),
        creator: C,
        f: F,
    ) -> R
    where
        C: FnOnce() -> Cell,
        F: FnOnce(&Cell) -> R,
    {
        match Self::compare_coords(coords, node.cell.coordinates()) {
            Ordering::Less => {
                {
                    let left_read = node.left.read().expect("Lock poisoned");
                    if let Some(ref left_node) = *left_read {
                        return Self::find_or_create_recursive(left_node, coords, creator, f);
                    }
                }
                let mut left_write = node.left.write().expect("Lock poisoned");
                if let Some(ref mut left_node) = *left_write {
                    Self::find_or_create_recursive(left_node, coords, creator, f)
                } else {
                    let cell = creator();
                    let result = f(&cell);
                    *left_write = Some(Box::new(CellNode::new(cell)));
                    result
                }
            }
            Ordering::Greater => {
                {
                    let right_read = node.right.read().expect("Lock poisoned");
                    if let Some(ref right_node) = *right_read {
                        return Self::find_or_create_recursive(right_node, coords, creator, f);
                    }
                }
                let mut right_write = node.right.write().expect("Lock poisoned");
                if let Some(ref mut right_node) = *right_write {
                    Self::find_or_create_recursive(right_node, coords, creator, f)
                } else {
                    let cell = creator();
                    let result = f(&cell);
                    *right_write = Some(Box::new(CellNode::new(cell)));
                    result
                }
            }
            Ordering::Equal => f(&node.cell),
        }
    }
    */

    pub fn collect_all(
        &self,
        current_mask: usize,
        last_mask: usize,
        last_last_mask: usize,
        out: &mut Vec<((i128, i128), u8)>,
    ) {
        if let Some(ref node) = self.root {
            Self::collect_all_recursive(node, current_mask, last_mask, last_last_mask, out);
        }
    }

    fn collect_all_recursive(
        node: &CellNode,
        current_mask: usize,
        last_mask: usize,
        last_last_mask: usize,
        out: &mut Vec<((i128, i128), u8)>,
    ) {
        if let Some(state) = node
            .cell
            .presenter_view(current_mask, last_mask, last_last_mask)
        {
            out.push((node.cell.coordinates(), state));
        }

        if let Some(ref left_node) = node.left {
            Self::collect_all_recursive(left_node, current_mask, last_mask, last_last_mask, out);
        }

        if let Some(ref right_node) = node.right {
            Self::collect_all_recursive(right_node, current_mask, last_mask, last_last_mask, out);
        }
    }

    pub fn collect_in_rect(
        &self,
        min: (i128, i128),
        max: (i128, i128),
        current_mask: usize,
        last_mask: usize,
        last_last_mask: usize,
        out: &mut Vec<((i128, i128), u8)>,
    ) {
        if let Some(ref node) = self.root {
            Self::collect_in_rect_recursive(
                node,
                min,
                max,
                current_mask,
                last_mask,
                last_last_mask,
                out,
            );
        }
    }

    fn collect_in_rect_recursive(
        node: &CellNode,
        min: (i128, i128),
        max: (i128, i128),
        current_mask: usize,
        last_mask: usize,
        last_last_mask: usize,
        out: &mut Vec<((i128, i128), u8)>,
    ) {
        let coords = node.cell.coordinates();

        // Check if in rect
        if coords.0 >= min.0 && coords.0 <= max.0 && coords.1 >= min.1 && coords.1 <= max.1 {
            if let Some(state) = node
                .cell
                .presenter_view(current_mask, last_mask, last_last_mask)
            {
                out.push((coords, state));
            }
        }

        // Lexicographical pruning:
        if Self::compare_coords(coords, min) == Ordering::Greater {
            if let Some(ref left_node) = node.left {
                Self::collect_in_rect_recursive(
                    left_node,
                    min,
                    max,
                    current_mask,
                    last_mask,
                    last_last_mask,
                    out,
                );
            }
        }

        if Self::compare_coords(coords, max) == Ordering::Less {
            if let Some(ref right_node) = node.right {
                Self::collect_in_rect_recursive(
                    right_node,
                    min,
                    max,
                    current_mask,
                    last_mask,
                    last_last_mask,
                    out,
                );
            }
        }
    }

    /// Recursively resets neighbor counts for all cells in the tree.
    pub fn reset_counts(&self, mask: usize) {
        if let Some(ref node) = self.root {
            Self::reset_counts_recursive(node, mask);
        }
    }

    fn reset_counts_recursive(node: &CellNode, mask: usize) {
        node.cell.reset_neighbor_count(mask);

        if let Some(ref left) = node.left {
            Self::reset_counts_recursive(left, mask);
        }
        if let Some(ref right) = node.right {
            Self::reset_counts_recursive(right, mask);
        }
    }

    /// Unified commit and prune: calculates next state, notifies observer, and prunes if perma-dead.
    /// This is the "natural pruning" integrated into the simulation cycle.
    pub fn commit_and_prune<F>(&mut self, cur: usize, next: usize, last: usize, mut observer: F)
    where
        F: FnMut(&Cell, crate::cell::CellState),
    {
        Self::commit_and_prune_recursive(&mut self.root, cur, next, last, &mut observer);
    }

    fn commit_and_prune_recursive<F>(
        node_opt: &mut Option<Box<CellNode>>,
        cur: usize,
        next: usize,
        last: usize,
        observer: &mut F,
    ) where
        F: FnMut(&Cell, crate::cell::CellState),
    {
        if let Some(mut node) = node_opt.take() {
            // 1. Recurse first to maintain tree structure during potential deletion
            Self::commit_and_prune_recursive(&mut node.left, cur, next, last, observer);
            Self::commit_and_prune_recursive(&mut node.right, cur, next, last, observer);

            // 2. Calculate next state
            let next_state = node.cell.calculate_next_state(cur, next);

            // 3. Notify observer (for counters)
            observer(&node.cell, next_state);

            // 4. Natural Pruning: Remove if dead in all 3 generations
            if node.cell.is_permanently_dead() {
                *node_opt = node.delete();
            } else {
                *node_opt = Some(node);
            }
        }
    }

    pub fn write_cells_streaming<W: Write>(
        &self,
        current_mask: usize,
        last_mask: usize,
        last_last_mask: usize,
        writer: &mut W,
        hasher: &mut crc32fast::Hasher,
    ) -> std::io::Result<()> {
        if let Some(ref node) = self.root {
            Self::write_streaming_recursive(
                node,
                current_mask,
                last_mask,
                last_last_mask,
                writer,
                hasher,
            )?;
        }
        Ok(())
    }

    fn write_streaming_recursive<W: Write>(
        node: &CellNode,
        current_mask: usize,
        last_mask: usize,
        last_last_mask: usize,
        writer: &mut W,
        hasher: &mut crc32fast::Hasher,
    ) -> std::io::Result<()> {
        let (x, y) = node.cell.coordinates();
        if let Some(state) = node
            .cell
            .presenter_view(current_mask, last_mask, last_last_mask)
        {
            let mut buf = [0u8; 33];
            buf[0..16].copy_from_slice(&x.to_le_bytes());
            buf[16..32].copy_from_slice(&y.to_le_bytes());
            buf[32] = state;

            hasher.update(&buf);
            writer.write_all(&buf)?;
        }

        if let Some(ref left_node) = node.left {
            Self::write_streaming_recursive(
                left_node,
                current_mask,
                last_mask,
                last_last_mask,
                writer,
                hasher,
            )?;
        }

        if let Some(ref right_node) = node.right {
            Self::write_streaming_recursive(
                right_node,
                current_mask,
                last_mask,
                last_last_mask,
                writer,
                hasher,
            )?;
        }

        Ok(())
    }

    pub fn compare_coords(a: (i128, i128), b: (i128, i128)) -> Ordering {
        if a.0 != b.0 {
            a.0.cmp(&b.0)
        } else {
            a.1.cmp(&b.1)
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct SendUnitPtr(pub *const CellNode);
unsafe impl Send for SendUnitPtr {}
unsafe impl Sync for SendUnitPtr {}

#[derive(Copy, Clone, Debug)]
pub struct SendUnitMutPtr(pub *mut CellNode);
unsafe impl Send for SendUnitMutPtr {}
unsafe impl Sync for SendUnitMutPtr {}

/// A thread-safe wrapper for raw pointers to `RwLock<Option<Box<CellNode>>>`.
#[derive(Copy, Clone, Debug)]
pub struct SendLockUnitPtr(pub *const RwLock<Option<Box<CellNode>>>);
unsafe impl Send for SendLockUnitPtr {}
unsafe impl Sync for SendLockUnitPtr {}
