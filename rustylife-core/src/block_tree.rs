use crate::cell::CellState;
use std::cmp::Ordering;

/// A block of 8x8 cells, packed into 3x 64-bit integers for history.
///
/// This struct maintains history for 3 generations (Current, Last, Next/LastLast)
/// using a circular buffer approach compatible with `SimulationMasks`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block8x8 {
    // 3 bitboards acting as a circular buffer.
    // Indexing follows the global mask logic (log2(mask)).
    // Mask 1 (001) -> boards[0]
    // Mask 2 (010) -> boards[1]
    // Mask 4 (100) -> boards[2]
    pub boards: [u64; 3],
}

impl Block8x8 {
    pub fn new() -> Self {
        Self { boards: [0; 3] }
    }

    /// Set a bit in the block for a specific mask index.
    pub fn set_bit(&mut self, local_x: u8, local_y: u8, mask_idx: usize, state: bool) {
        let bit_index = (local_y as usize * 8) + local_x as usize;
        if state {
            self.boards[mask_idx] |= 1 << bit_index;
        } else {
            self.boards[mask_idx] &= !(1 << bit_index);
        }
    }

    /// Get a bit from the block for a specific mask index.
    pub fn get_bit(&self, local_x: u8, local_y: u8, mask_idx: usize) -> bool {
        let bit_index = (local_y as usize * 8) + local_x as usize;
        (self.boards[mask_idx] & (1 << bit_index)) != 0
    }

    /// Advances the state of the block by one generation.
    ///
    /// # Arguments
    /// * `n_mask`, `s_mask`, `w_mask`, `e_mask`: Neighbor bits from adjacent blocks.
    /// * `nw`, `ne`, `sw`, `se`: Corner bits.
    /// * `current_idx`: Index of the current generation in `boards`.
    /// * `next_idx`: Index where the next generation should be written.
    pub fn step(
        &mut self,
        n_mask: u64,
        s_mask: u64,
        w_mask: u64,
        e_mask: u64,
        nw_mask: u64,
        ne_mask: u64,
        sw_mask: u64,
        se_mask: u64,
        current_idx: usize,
        next_idx: usize,
    ) -> (u8, u8, u8, u8, bool) {
        let center = self.boards[current_idx];

        // Bitwise logic adapted from original experiment
        const COL_7_MASK: u64 = 0x7F7F7F7F7F7F7F7F; // Clear MSB of each byte
        const COL_0_MASK: u64 = 0xFEFEFEFEFEFEFEFE; // Clear LSB of each byte

        let n = (center << 8) | n_mask;
        let s = (center >> 8) | s_mask;
        // West uses << 1 (Bit 0 -> Bit 1). East uses >> 1 (Bit 2 -> Bit 1).
        let w = ((center & COL_7_MASK) << 1) | w_mask;
        let e = ((center & COL_0_MASK) >> 1) | e_mask;

        // Corners: explicitly combine derived internal corners with external corner masks
        // Internal NE: (n & col7) << 1. External NE: ne_mask (shifted to correct position by caller? No, mask should be aligned).
        // Wait, caller passes 64-bit mask.
        // For corners, the mask should have the single bit set at the correct position (e.g. 0, 7, 56, 63).
        // But `ne` variable needs to be a full 64-bit neighborhood mask.
        // It needs `ne` bit for EVERY cell.
        // Most cells get NE from `n` shifted or `e` shifted.
        // Only the edgemost cells need external data.

        let ne_internal = (n & COL_0_MASK) >> 1;
        let nw_internal = (n & COL_7_MASK) << 1;
        let se_internal = (s & COL_0_MASK) >> 1;
        let sw_internal = (s & COL_7_MASK) << 1;

        // Combine with external masks
        // Ensure masks only provide the missing bits to avoid corruption?
        // Caller provides single bits. OR them in.

        let _ne = ne_internal | e_mask << 8 | ne_mask; // e_mask<<8 covers right edge. ne_mask covers BL corner of NE block? No.
        // ne_mask is the NE Neighbor Data.
        // For cell (7,0), we need NE neighbor (-1, -1) relative to it?
        // Cell (7,0) is Top-Right. NE neighbor is at (8, -1).
        // This is outside.
        // `ne_mask` should provide this.

        // Simplified: Trust the masks provided by caller to fill the gaps.
        // The `internal` shifts leave gaps at edges.
        // `e_mask` fills Right Edge.
        // `n_mask` fills Top Edge.
        // `ne_mask` fills Top-Right Corner.

        // Let's assume masks are pre-shifted by caller if needed, or simply placed correctly.
        // In engine.rs, we passed `1u64 << 56` etc.
        // These are single bits.
        // Just OR them in.

        // NE needs East Edge (shifted North) and NE Corner
        let ne = ne_internal | (e_mask << 8) | ne_mask;
        // NW needs West Edge (shifted North) and NW Corner
        let nw = nw_internal | (w_mask << 8) | nw_mask;
        // SE needs East Edge (shifted South) and SE Corner
        let se = se_internal | (e_mask >> 8) | se_mask;
        // SW needs West Edge (shifted South) and SW Corner
        let sw = sw_internal | (w_mask >> 8) | sw_mask;

        // Full Adder Chain (B3/S23)
        // Layer 1
        let s1 = n ^ s;
        let c1 = n & s;
        let s2 = e ^ w;
        let c2 = e & w;
        let s3 = ne ^ nw;
        let c3 = ne & nw;
        let s4 = se ^ sw;
        let c4 = se & sw;

        // Layer 2
        let s12 = s1 ^ s2;
        let c12 = s1 & s2;
        let s34 = s3 ^ s4;
        let c34 = s3 & s4;

        let s12_34 = s12 ^ s34; // Bit 0
        let c_s12_s34 = s12 & s34;

        // Layer 3 (Carries)
        let k1 = c1 ^ c2;
        let l1 = c1 & c2;
        let k2 = c3 ^ c4;
        let l2 = c3 & c4;
        let k3 = c12 ^ c34;
        let l3 = c12 & c34;

        let sum_bit_1_partial = k1 ^ k2;
        let carry_k1k2 = k1 & k2;

        let sum_bit_1 = sum_bit_1_partial ^ k3 ^ c_s12_s34;

        // Weight 4 check (Overcrowding)
        let carry_k3 = (sum_bit_1_partial & k3) | ((sum_bit_1_partial ^ k3) & c_s12_s34);
        let weight_4_mask = l1 | l2 | l3 | carry_k1k2 | carry_k3;

        // Alive if 3, or (2 and Alive)
        // 3: Bit0=1, Bit1=1
        // 2: Bit0=0, Bit1=1
        // Result = (!Mask4 & Bit1) & (Bit0 | Center)

        let next_state = (!weight_4_mask & sum_bit_1) & (s12_34 | center);
        self.boards[next_idx] = next_state;

        // Metrics Calculation
        let pop = next_state.count_ones() as u8;

        // Born: Alive in NEXT but NOT in CURRENT

        // Born: Alive in NEXT but NOT in CURRENT
        let born = (next_state & !center).count_ones() as u8;

        // Died: Alive in CURRENT but NOT in NEXT
        let died = (center & !next_state).count_ones() as u8;

        // Work = Union of all 3 buffers (Current | Last | Next)
        let work = (self.boards[0] | self.boards[1] | self.boards[2]).count_ones() as u8;
        let is_dead = work == 0;

        (pop, born, died, work, is_dead)
    }

    pub fn is_dead(&self) -> bool {
        self.boards[0] == 0 && self.boards[1] == 0 && self.boards[2] == 0
    }

    /// Returns the bounding box of living cells in the given state index (0, 1, or 2).
    /// Coordinates are relative to the block's origin (0..7).
    pub fn exact_bounds_in_state(&self, state_idx: usize) -> Option<((i128, i128), (i128, i128))> {
        let state = self.boards[state_idx];
        if state == 0 {
            return None;
        }

        // Y-bounds: trailing/leading zeros on the 64-bit word correspond to rows.
        let min_y = (state.trailing_zeros() / 8) as i128;
        let max_y = (63 - state.leading_zeros()) as i128 / 8;

        // X-bounds: collapse all rows into one 8-bit mask.
        // We use a simple loop or a sequence of ORs.
        let mut row_mask = (state & 0xFF) as u8;
        for i in 1..8 {
            row_mask |= ((state >> (i * 8)) & 0xFF) as u8;
        }

        let min_x = row_mask.trailing_zeros() as i128;
        let max_x = 7 - row_mask.leading_zeros() as i128; // u8.leading_zeros() returns 0..8

        Some(((min_x, min_y), (max_x, max_y)))
    }
}

pub type BlockIndex = u32;

#[derive(Debug, Clone)]
pub struct BlockNode {
    pub bx: i128,
    pub by: i128,
    pub block: Block8x8,
    pub left: Option<BlockIndex>,
    pub right: Option<BlockIndex>,
}

impl BlockNode {
    pub fn new(bx: i128, by: i128) -> Self {
        Self {
            bx,
            by,
            block: Block8x8::new(),
            left: None,
            right: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct BlockArena {
    pub nodes: Vec<BlockNode>,
    // free_list removed as we use uniform pruning (compaction)
}

impl BlockArena {
    pub fn new() -> Self {
        Self {
            nodes: Vec::with_capacity(1024),
        }
    }

    pub fn alloc(&mut self, bx: i128, by: i128) -> BlockIndex {
        // Enforce fixed chunk growth to avoid exponential doubling behavior
        if self.nodes.len() == self.nodes.capacity() {
            self.nodes.reserve(1024);
        }

        let idx = self.nodes.len() as BlockIndex;
        self.nodes.push(BlockNode::new(bx, by));
        idx
    }

    pub fn get(&self, idx: BlockIndex) -> &BlockNode {
        &self.nodes[idx as usize]
    }

    pub fn get_mut(&mut self, idx: BlockIndex) -> &mut BlockNode {
        &mut self.nodes[idx as usize]
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
    }
}

pub struct BlockTree {
    pub root: Option<BlockIndex>,
    pub arena: BlockArena,
}

impl BlockTree {
    pub fn new() -> Self {
        Self {
            root: None,
            arena: BlockArena::new(),
        }
    }

    pub fn bounds(&self, mask: usize) -> Option<((i128, i128), (i128, i128))> {
        let mask_idx = Self::mask_to_index(mask);
        let mut min_x = i128::MAX;
        let mut min_y = i128::MAX;
        let mut max_x = i128::MIN;
        let mut max_y = i128::MIN;
        let mut found = false;

        for node in &self.arena.nodes {
            if node.block.boards[mask_idx] == 0 {
                continue;
            }

            // Improve precision by checking bits
            let base_x = node.bx << 3;
            let base_y = node.by << 3;
            let board = node.block.boards[mask_idx];

            for i in 0..64 {
                if (board & (1 << i)) != 0 {
                    let lx = (i % 8) as i128;
                    let ly = (i / 8) as i128;
                    let x = base_x + lx;
                    let y = base_y + ly;

                    if x < min_x {
                        min_x = x;
                    }
                    if x > max_x {
                        max_x = x;
                    }
                    if y < min_y {
                        min_y = y;
                    }
                    if y > max_y {
                        max_y = y;
                    }
                    found = true;
                }
            }
        }

        if found {
            Some(((min_x, min_y), (max_x, max_y)))
        } else {
            None
        }
    }

    pub fn clear(&mut self) {
        self.root = None;
        self.arena.clear();
    }

    /// Converts global cell coordinates to block coordinates and local offsets.
    fn coords_to_block(x: i128, y: i128) -> (i128, i128, u8, u8) {
        let bx = x >> 3;
        let by = y >> 3;
        // Local coords: need to handle negative numbers correctly for modulo.
        // Rust's % operator preserves sign. We want Euclidean modulo.
        // x & 7 is sufficient if we assume 2's complement representation aligns (which it does for bitwise logic).
        let lx = (x & 7) as u8;
        let ly = (y & 7) as u8;
        (bx, by, lx, ly)
    }

    /// Maps a mask (1, 2, 4) to an array index (0, 1, 2).
    fn mask_to_index(mask: usize) -> usize {
        match mask {
            1 => 0,
            2 => 1,
            4 => 2,
            _ => 0, // Fallback, shouldn't happen
        }
    }

    pub fn set_cell(&mut self, x: i128, y: i128, mask: usize, state: CellState) {
        let (bx, by, lx, ly) = Self::coords_to_block(x, y);
        let mask_idx = Self::mask_to_index(mask);

        // BST Insert/Find
        if self.root.is_none() {
            let root_idx = self.arena.alloc(bx, by);
            self.root = Some(root_idx);
        }

        let mut curr_idx = self.root.expect("Root should exist");
        loop {
            // Scope the borrow of 'node' so it ends before we need to mutate 'self.arena'
            let (order, next_idx) = {
                let node = self.arena.get(curr_idx);
                let order = Self::compare_coords(bx, by, node.bx, node.by);
                match order {
                    Ordering::Equal => (Ordering::Equal, None),
                    Ordering::Less => (Ordering::Less, node.left),
                    Ordering::Greater => (Ordering::Greater, node.right),
                }
            };

            match order {
                Ordering::Equal => {
                    break;
                }
                Ordering::Less => {
                    if let Some(left) = next_idx {
                        curr_idx = left;
                    } else {
                        // Insert Left
                        let new_node = self.arena.alloc(bx, by);
                        self.arena.get_mut(curr_idx).left = Some(new_node);
                        curr_idx = new_node;
                        break;
                    }
                }
                Ordering::Greater => {
                    if let Some(right) = next_idx {
                        curr_idx = right;
                    } else {
                        // Insert Right
                        let new_node = self.arena.alloc(bx, by);
                        self.arena.get_mut(curr_idx).right = Some(new_node);
                        curr_idx = new_node;
                        break;
                    }
                }
            }
        }

        // Now curr_idx points to the node (existing or newly created)
        let node = self.arena.get_mut(curr_idx);
        node.block
            .set_bit(lx, ly, mask_idx, state == CellState::Alive);
    }

    pub fn ensure_block(&mut self, bx: i128, by: i128) -> BlockIndex {
        if self.root.is_none() {
            let root_idx = self.arena.alloc(bx, by);
            self.root = Some(root_idx);
            return root_idx;
        }

        let mut curr_idx = self.root.unwrap();
        loop {
            let (order, next_idx) = {
                let node = self.arena.get(curr_idx);
                let order = Self::compare_coords(bx, by, node.bx, node.by);
                match order {
                    Ordering::Equal => (Ordering::Equal, None),
                    Ordering::Less => (Ordering::Less, node.left),
                    Ordering::Greater => (Ordering::Greater, node.right),
                }
            };

            match order {
                Ordering::Equal => return curr_idx,
                Ordering::Less => {
                    if let Some(left) = next_idx {
                        curr_idx = left;
                    } else {
                        let new_node = self.arena.alloc(bx, by);
                        self.arena.get_mut(curr_idx).left = Some(new_node);
                        return new_node;
                    }
                }
                Ordering::Greater => {
                    if let Some(right) = next_idx {
                        curr_idx = right;
                    } else {
                        let new_node = self.arena.alloc(bx, by);
                        self.arena.get_mut(curr_idx).right = Some(new_node);
                        return new_node;
                    }
                }
            }
        }
    }

    pub fn get_cell(&self, x: i128, y: i128, mask: usize) -> CellState {
        let (bx, by, lx, ly) = Self::coords_to_block(x, y);
        let mask_idx = Self::mask_to_index(mask);

        let mut curr = self.root;
        while let Some(idx) = curr {
            let node = self.arena.get(idx);
            match Self::compare_coords(bx, by, node.bx, node.by) {
                Ordering::Equal => {
                    return if node.block.get_bit(lx, ly, mask_idx) {
                        CellState::Alive
                    } else {
                        CellState::Dead
                    };
                }
                Ordering::Less => curr = node.left,
                Ordering::Greater => curr = node.right,
            }
        }
        CellState::Dead
    }

    fn compare_coords(an_x: i128, an_y: i128, bn_x: i128, bn_y: i128) -> Ordering {
        if an_y == bn_y {
            an_x.cmp(&bn_x)
        } else {
            an_y.cmp(&bn_y)
        }
    }

    /// Collects all active cells matching the masks.
    /// This is used for rendering and serialization.
    pub fn collect_cells(
        &self,
        current_mask: usize,
        last_mask: usize,
        last_last_mask: usize,
        out: &mut Vec<((i128, i128), u8)>,
    ) {
        let c_idx = Self::mask_to_index(current_mask);
        let l_idx = Self::mask_to_index(last_mask);
        let _ll_idx = Self::mask_to_index(last_last_mask);

        // Iterate direct arena for performance (skipping tree traversal)
        for node in &self.arena.nodes {
            // Optimization: If block is completely empty in all masks, skip?
            // Block doesn't track emptiness, would need to check u64s.
            if node.block.boards[0] == 0 && node.block.boards[1] == 0 && node.block.boards[2] == 0 {
                continue;
            }

            let bx_world = node.bx << 3;
            let by_world = node.by << 3;

            for ly in 0..8 {
                for lx in 0..8 {
                    let bit_index = (ly * 8) + lx;
                    let bit_mask = 1 << bit_index;

                    let c = (node.block.boards[c_idx] & bit_mask) != 0;
                    let l = (node.block.boards[l_idx] & bit_mask) != 0;
                    // let ll = (node.block.boards[ll_idx] & bit_mask) != 0; // Unused for now

                    let state_byte = if c {
                        if l {
                            3 // Stable
                        } else {
                            2 // Born
                        }
                    } else if l {
                        1 // Dying
                    } else {
                        0 // Ghost / Dead
                    };

                    if state_byte != 0 {
                        out.push(((bx_world + lx as i128, by_world + ly as i128), state_byte));
                    }
                }
            }
        }
    }

    pub fn collect_cells_in_rect(
        &self,
        min: (i128, i128),
        max: (i128, i128),
        current_mask: usize,
        last_mask: usize,
        last_last_mask: usize,
        out: &mut Vec<((i128, i128), u8)>,
    ) {
        let c_idx = Self::mask_to_index(current_mask);
        let l_idx = Self::mask_to_index(last_mask);
        let _ll_idx = Self::mask_to_index(last_last_mask);

        // For Rect, tree traversal might be faster if arena is huge?
        // But BST is sorted by Coordinates. Range query is possible.
        // For now, let's use global arena scan for simplicity because "viewport" is usually small/specific
        // OR viewport is the whole screen.
        // Actually, if we want efficiency, we should implement range query on BST.
        // But iterating a Vec is extremely fast.
        // Let's iterate Vec and filter.

        for node in &self.arena.nodes {
            let bx_world = node.bx << 3;
            let by_world = node.by << 3;

            // Check if block intersects rect
            if bx_world + 8 < min.0 || bx_world > max.0 || by_world + 8 < min.1 || by_world > max.1
            {
                continue;
            }

            // Iterate bits
            for ly in 0..8 {
                let y = by_world + ly as i128;
                if y < min.1 || y > max.1 {
                    continue;
                }

                for lx in 0..8 {
                    let x = bx_world + lx as i128;
                    if x < min.0 || x > max.0 {
                        continue;
                    }

                    let bit_index = (ly * 8) + lx;
                    let bit_mask = 1 << bit_index;

                    let c = (node.block.boards[c_idx] & bit_mask) != 0;
                    let l = (node.block.boards[l_idx] & bit_mask) != 0;
                    // let ll = (node.block.boards[ll_idx] & bit_mask) != 0; // Unused for now

                    let state_byte = if c {
                        if l {
                            3 // Stable
                        } else {
                            2 // Born
                        }
                    } else if l {
                        1 // Dying
                    } else {
                        0 // Ghost / Dead
                    };

                    if state_byte != 0 {
                        out.push(((x, y), state_byte));
                    }
                }
            }
        }
    }

    pub fn population(&self, mask: u8) -> u64 {
        let board_idx = mask.trailing_zeros() as usize;
        if board_idx >= 3 {
            // Fallback or error? If mask is 0, trailing_zeros is 32/64.
            // If mask is not power of 2, it picks lowest bit.
            // Assuming mask is valid 1, 2, 4.
            // If mask is 0, we shouldn't be here.
            return 0;
        }

        let mut total = 0;
        for (_i, node) in self.arena.nodes.iter().enumerate() {
            if !node.block.is_dead() {
                let count = node.block.boards[board_idx].count_ones() as u64;
                total += count;
            }
        }
        total
    }

    pub fn prune(&mut self) {
        // Optimized Aggressive Pruning: O(N)
        // 1. Traverse existing tree In-Order to collect ALIVE blocks.
        //    Since it's a BST, in-order traversal yields nodes sorted by coordinate.
        // 2. Clear arena.
        // 3. Bulk-load the sorted blocks into a new balanced tree.

        let mut alive_blocks = Vec::with_capacity(self.arena.nodes.len());
        self.collect_alive_in_order(self.root, &mut alive_blocks);

        if alive_blocks.is_empty() {
            self.clear();
            return;
        }

        // Use the naturally sorted data to rebuild
        self.arena.clear();
        self.root = None;

        // Pre-allocate headroom
        self.arena.nodes.reserve(alive_blocks.len() + 1024);

        self.root = Self::bulk_load(&alive_blocks, &mut self.arena);
    }

    fn collect_alive_in_order(&self, node_idx: Option<BlockIndex>, out: &mut Vec<BlockNode>) {
        if let Some(idx) = node_idx {
            // Recursive descent
            // Note: recursion depth limited by tree height.
            // If tree is very unbalanced (linked list), this could blow stack.
            // But we rebuild as balanced, so it should stay manageable.
            let node = &self.arena.nodes[idx as usize];

            // Left
            self.collect_alive_in_order(node.left, out);

            // Center (Self) - Filter DEAD nodes here
            if !node.block.is_dead() {
                out.push(node.clone());
            }

            // Right
            self.collect_alive_in_order(node.right, out);
        }
    }

    fn bulk_load(nodes: &[BlockNode], arena: &mut BlockArena) -> Option<BlockIndex> {
        if nodes.is_empty() {
            return None;
        }

        let mid = nodes.len() / 2;
        let node_data = &nodes[mid];

        // Alloc in arena
        let idx = arena.alloc(node_data.bx, node_data.by);
        // Copy block data
        arena.nodes[idx as usize].block = node_data.block;

        // Recurse
        arena.nodes[idx as usize].left = Self::bulk_load(&nodes[0..mid], arena);
        arena.nodes[idx as usize].right = Self::bulk_load(&nodes[mid + 1..], arena);

        Some(idx)
    }

    // collect_metric_stats removed (calculated in step)
}
