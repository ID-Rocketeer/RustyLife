use crate::cell::Cell;
use std::cmp::Ordering;
use std::io::Write;

/// A stable index into the NodeArena.
pub type NodeIndex = u32;

/// A node in the coordinate-ordered binary search tree.
///
/// Uses indices instead of Box/pointers to allow for efficient pooling and
/// to avoid heap fragmentation during high-frequency birth/death cycles.
#[derive(Debug, Clone)]
pub struct CellNode {
    pub cell: Cell,
    pub left: Option<NodeIndex>,
    pub right: Option<NodeIndex>,
}

impl CellNode {
    pub fn new(cell: Cell) -> Self {
        Self {
            cell,
            left: None,
            right: None,
        }
    }
}

/// A compact, index-based storage for CellNodes.
///
/// Each tree owns its arena to ensure thread-safe localized access within bucket locks.
#[derive(Debug, Default)]
pub struct NodeArena {
    nodes: Vec<CellNode>,
    free_list: Vec<NodeIndex>,
}

impl NodeArena {
    pub fn new() -> Self {
        Self {
            nodes: Vec::with_capacity(1024),
            free_list: Vec::with_capacity(128),
        }
    }

    pub fn alloc(&mut self, cell: Cell) -> NodeIndex {
        if let Some(idx) = self.free_list.pop() {
            self.nodes[idx as usize] = CellNode::new(cell);
            idx
        } else {
            let idx = self.nodes.len() as NodeIndex;
            self.nodes.push(CellNode::new(cell));
            idx
        }
    }

    pub fn free(&mut self, idx: NodeIndex) {
        self.free_list.push(idx);
    }

    pub fn get(&self, idx: NodeIndex) -> &CellNode {
        &self.nodes[idx as usize]
    }

    pub fn get_mut(&mut self, idx: NodeIndex) -> &mut CellNode {
        &mut self.nodes[idx as usize]
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.free_list.clear();
    }
}

pub struct CellTree {
    pub root: Option<NodeIndex>,
    pub arena: NodeArena,
}

impl CellTree {
    pub fn new() -> Self {
        Self {
            root: None,
            arena: NodeArena::new(),
        }
    }

    pub fn insert(&mut self, cell: Cell) {
        let coords = cell.coordinates();
        self.find_or_create_and_apply(coords, || cell, |_| ());
    }

    pub fn clear(&mut self) {
        self.root = None;
        self.arena.clear();
    }

    pub fn find_and_apply<F, R>(&self, coords: (i128, i128), f: F) -> Option<R>
    where
        F: FnOnce(&Cell) -> R,
    {
        self.root
            .and_then(|idx| self.find_recursive(idx, coords, f))
    }

    fn find_recursive<F, R>(&self, idx: NodeIndex, coords: (i128, i128), f: F) -> Option<R>
    where
        F: FnOnce(&Cell) -> R,
    {
        let node = self.arena.get(idx);
        match Self::compare_coords(coords, node.cell.coordinates()) {
            Ordering::Less => node
                .left
                .and_then(|left_idx| self.find_recursive(left_idx, coords, f)),
            Ordering::Greater => node
                .right
                .and_then(|right_idx| self.find_recursive(right_idx, coords, f)),
            Ordering::Equal => Some(f(&node.cell)),
        }
    }

    pub fn find_or_create_and_apply<F, C, R>(&mut self, coords: (i128, i128), creator: C, f: F) -> R
    where
        C: FnOnce() -> Cell,
        F: FnOnce(&mut Cell) -> R,
    {
        Self::find_or_create_recursive(&mut self.arena, &mut self.root, coords, creator, f)
    }

    fn find_or_create_recursive<F, C, R>(
        arena: &mut NodeArena,
        node_idx_opt: &mut Option<NodeIndex>,
        coords: (i128, i128),
        creator: C,
        f: F,
    ) -> R
    where
        C: FnOnce() -> Cell,
        F: FnOnce(&mut Cell) -> R,
    {
        if let Some(idx) = *node_idx_opt {
            let node = arena.get_mut(idx);
            match Self::compare_coords(coords, node.cell.coordinates()) {
                Ordering::Equal => f(&mut node.cell),
                Ordering::Less => {
                    // Re-borrow to avoid lifetime issues
                    let mut left = arena.get_mut(idx).left;
                    let res = Self::find_or_create_recursive(arena, &mut left, coords, creator, f);
                    arena.get_mut(idx).left = left;
                    res
                }
                Ordering::Greater => {
                    let mut right = arena.get_mut(idx).right;
                    let res = Self::find_or_create_recursive(arena, &mut right, coords, creator, f);
                    arena.get_mut(idx).right = right;
                    res
                }
            }
        } else {
            let new_idx = arena.alloc(creator());
            *node_idx_opt = Some(new_idx);
            f(&mut arena.get_mut(new_idx).cell)
        }
    }

    pub fn apply_batch<C, F>(&mut self, sorted_coords: &[(i128, i128)], creator: C, applicator: F)
    where
        C: Fn(i128, i128) -> Cell,
        F: Fn(&mut Cell),
    {
        Self::apply_recursive(
            &mut self.arena,
            &mut self.root,
            sorted_coords,
            &creator,
            &applicator,
        );
    }

    fn apply_recursive<C, F>(
        arena: &mut NodeArena,
        node_idx_opt: &mut Option<NodeIndex>,
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

        if let Some(idx) = *node_idx_opt {
            let n_coords = arena.get(idx).cell.coordinates();

            let start_idx =
                coords.partition_point(|c| Self::compare_coords(*c, n_coords) == Ordering::Less);

            let mut end_idx = start_idx;
            while end_idx < coords.len() && coords[end_idx] == n_coords {
                end_idx += 1;
            }

            let mut left = arena.get(idx).left;
            Self::apply_recursive(arena, &mut left, &coords[..start_idx], creator, applicator);
            arena.get_mut(idx).left = left;

            for _ in start_idx..end_idx {
                applicator(&mut arena.get_mut(idx).cell);
            }

            let mut right = arena.get(idx).right;
            Self::apply_recursive(arena, &mut right, &coords[end_idx..], creator, applicator);
            arena.get_mut(idx).right = right;
        } else {
            *node_idx_opt = Self::build_from_coords(arena, coords, creator, applicator);
        }
    }

    fn build_from_coords<C, F>(
        arena: &mut NodeArena,
        coords: &[(i128, i128)],
        creator: &C,
        applicator: &F,
    ) -> Option<NodeIndex>
    where
        C: Fn(i128, i128) -> Cell,
        F: Fn(&mut Cell),
    {
        if coords.is_empty() {
            return None;
        }

        let mid = coords.len() / 2;
        let mid_coord = coords[mid];

        let mut start = mid;
        while start > 0 && coords[start - 1] == mid_coord {
            start -= 1;
        }

        let mut end = mid + 1;
        while end < coords.len() && coords[end] == mid_coord {
            end += 1;
        }

        let new_idx = arena.alloc(creator(mid_coord.0, mid_coord.1));

        for _ in start..end {
            applicator(&mut arena.get_mut(new_idx).cell);
        }

        let left = Self::build_from_coords(arena, &coords[..start], creator, applicator);
        arena.get_mut(new_idx).left = left;

        let right = Self::build_from_coords(arena, &coords[end..], creator, applicator);
        arena.get_mut(new_idx).right = right;

        Some(new_idx)
    }

    pub fn collect_all(
        &self,
        current_mask: usize,
        last_mask: usize,
        last_last_mask: usize,
        out: &mut Vec<((i128, i128), u8)>,
    ) {
        if let Some(idx) = self.root {
            self.collect_all_recursive(idx, current_mask, last_mask, last_last_mask, out);
        }
    }

    fn collect_all_recursive(
        &self,
        idx: NodeIndex,
        current_mask: usize,
        last_mask: usize,
        last_last_mask: usize,
        out: &mut Vec<((i128, i128), u8)>,
    ) {
        let node = self.arena.get(idx);
        if let Some(state) = node
            .cell
            .presenter_view(current_mask, last_mask, last_last_mask)
        {
            out.push((node.cell.coordinates(), state));
        }

        if let Some(left_idx) = node.left {
            self.collect_all_recursive(left_idx, current_mask, last_mask, last_last_mask, out);
        }

        if let Some(right_idx) = node.right {
            self.collect_all_recursive(right_idx, current_mask, last_mask, last_last_mask, out);
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
        if let Some(idx) = self.root {
            self.collect_in_rect_recursive(
                idx,
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
        &self,
        idx: NodeIndex,
        min: (i128, i128),
        max: (i128, i128),
        current_mask: usize,
        last_mask: usize,
        last_last_mask: usize,
        out: &mut Vec<((i128, i128), u8)>,
    ) {
        let node = self.arena.get(idx);
        let coords = node.cell.coordinates();

        if coords.0 >= min.0 && coords.0 <= max.0 && coords.1 >= min.1 && coords.1 <= max.1 {
            if let Some(state) = node
                .cell
                .presenter_view(current_mask, last_mask, last_last_mask)
            {
                out.push((coords, state));
            }
        }

        if Self::compare_coords(coords, min) == Ordering::Greater {
            if let Some(left_idx) = node.left {
                self.collect_in_rect_recursive(
                    left_idx,
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
            if let Some(right_idx) = node.right {
                self.collect_in_rect_recursive(
                    right_idx,
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

    pub fn reset_all_counts(&self) {
        if let Some(idx) = self.root {
            self.reset_all_counts_recursive(idx);
        }
    }

    fn reset_all_counts_recursive(&self, idx: NodeIndex) {
        let node = self.arena.get(idx);
        node.cell.reset_all_counts();

        if let Some(left) = node.left {
            self.reset_all_counts_recursive(left);
        }
        if let Some(right) = node.right {
            self.reset_all_counts_recursive(right);
        }
    }

    pub fn commit_and_prune<F>(&mut self, cur: usize, next: usize, last: usize, mut observer: F)
    where
        F: FnMut(&Cell, crate::cell::CellState),
    {
        Self::commit_and_prune_recursive(
            &mut self.arena,
            &mut self.root,
            cur,
            next,
            last,
            &mut observer,
        );
    }

    fn commit_and_prune_recursive<F>(
        arena: &mut NodeArena,
        node_idx_opt: &mut Option<NodeIndex>,
        cur: usize,
        next: usize,
        last: usize,
        observer: &mut F,
    ) where
        F: FnMut(&Cell, crate::cell::CellState),
    {
        if let Some(idx) = node_idx_opt.take() {
            // Recurse first
            let mut left = arena.get(idx).left;
            Self::commit_and_prune_recursive(arena, &mut left, cur, next, last, observer);
            arena.get_mut(idx).left = left;

            let mut right = arena.get(idx).right;
            Self::commit_and_prune_recursive(arena, &mut right, cur, next, last, observer);
            arena.get_mut(idx).right = right;

            // Calculate next state
            let next_state = arena.get(idx).cell.calculate_next_state(cur, next);
            observer(&arena.get(idx).cell, next_state);

            // Natural Pruning: Remove if dead in all 3 generations
            if arena.get(idx).cell.is_permanently_dead() {
                let left = arena.get(idx).left;
                let right = arena.get(idx).right;
                *node_idx_opt = Self::delete_node(arena, left, right);
                arena.free(idx);
            } else {
                *node_idx_opt = Some(idx);
            }
        }
    }

    fn delete_node(
        arena: &mut NodeArena,
        left: Option<NodeIndex>,
        right: Option<NodeIndex>,
    ) -> Option<NodeIndex> {
        match (left, right) {
            (None, None) => None,
            (Some(l), None) => Some(l),
            (None, Some(r)) => Some(r),
            (Some(l), Some(r)) => {
                let (min_idx, new_right) = Self::extract_min(arena, r);
                arena.get_mut(min_idx).left = Some(l);
                arena.get_mut(min_idx).right = new_right;
                Some(min_idx)
            }
        }
    }

    fn extract_min(arena: &mut NodeArena, idx: NodeIndex) -> (NodeIndex, Option<NodeIndex>) {
        if let Some(left_idx) = arena.get(idx).left {
            let (min, replacement) = Self::extract_min(arena, left_idx);
            arena.get_mut(idx).left = replacement;
            (min, Some(idx))
        } else {
            let right = arena.get(idx).right;
            (idx, right)
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
        if let Some(idx) = self.root {
            self.write_streaming_recursive(
                idx,
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
        &self,
        idx: NodeIndex,
        current_mask: usize,
        last_mask: usize,
        last_last_mask: usize,
        writer: &mut W,
        hasher: &mut crc32fast::Hasher,
    ) -> std::io::Result<()> {
        let node = self.arena.get(idx);
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

        if let Some(left_idx) = node.left {
            self.write_streaming_recursive(
                left_idx,
                current_mask,
                last_mask,
                last_last_mask,
                writer,
                hasher,
            )?;
        }

        if let Some(right_idx) = node.right {
            self.write_streaming_recursive(
                right_idx,
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
