use crate::cell::Cell;
use std::cmp::Ordering;
use std::sync::RwLock;

pub struct CellNode {
    pub cell: Cell,
    pub left: RwLock<Option<Box<CellNode>>>,
    pub right: RwLock<Option<Box<CellNode>>>,
}

impl CellNode {
    pub fn new(cell: Cell) -> Self {
        Self {
            cell,
            left: RwLock::new(None),
            right: RwLock::new(None),
        }
    }

    pub fn delete(self: Box<Self>) -> Option<Box<Self>> {
        let left = self.left.into_inner().expect("Lock poisoned");
        let right = self.right.into_inner().expect("Lock poisoned");

        match (left, right) {
            (None, None) => None,
            (Some(l), None) => Some(l),
            (None, Some(r)) => Some(r),
            (Some(l), Some(r)) => {
                let (min_node, new_right) = Self::extract_min(r);
                let mut replacement = min_node;
                *replacement.left.get_mut().expect("Lock poisoned") = Some(l);
                *replacement.right.get_mut().expect("Lock poisoned") = new_right;
                Some(replacement)
            }
        }
    }

    fn extract_min(mut node: Box<Self>) -> (Box<Self>, Option<Box<Self>>) {
        // We use get_mut here because we have ownership of the Box
        if node.left.get_mut().expect("Lock poisoned").is_some() {
            let left_node = node.left.get_mut().unwrap().take().unwrap();
            let (min, replacement) = Self::extract_min(left_node);
            *node.left.get_mut().unwrap() = replacement;
            (min, Some(node))
        } else {
            let right = node.right.get_mut().expect("Lock poisoned").take();
            (node, right)
        }
    }
}

pub struct CellTree {
    pub root: RwLock<Option<Box<CellNode>>>,
}

impl CellTree {
    pub fn new() -> Self {
        Self {
            root: RwLock::new(None),
        }
    }

    pub fn insert(&self, cell: Cell) {
        let coords = cell.coordinates();
        self.find_or_create_and_apply(coords, || cell, |_| ());
    }

    pub fn find_and_apply<F, R>(&self, coords: (i128, i128), f: F) -> Option<R>
    where
        F: FnOnce(&Cell) -> R,
    {
        let root_lock = self.root.read().expect("Lock poisoned");
        root_lock
            .as_ref()
            .and_then(|node| Self::find_recursive(node, coords, f))
    }

    fn find_recursive<F, R>(node: &CellNode, coords: (i128, i128), f: F) -> Option<R>
    where
        F: FnOnce(&Cell) -> R,
    {
        match Self::compare_coords(coords, node.cell.coordinates()) {
            Ordering::Less => {
                let left_lock = node.left.read().expect("Lock poisoned");
                left_lock
                    .as_ref()
                    .and_then(|n| Self::find_recursive(n, coords, f))
            }
            Ordering::Greater => {
                let right_lock = node.right.read().expect("Lock poisoned");
                right_lock
                    .as_ref()
                    .and_then(|n| Self::find_recursive(n, coords, f))
            }
            Ordering::Equal => Some(f(&node.cell)),
        }
    }

    pub fn find_or_create_and_apply<F, C, R>(&self, coords: (i128, i128), creator: C, f: F) -> R
    where
        C: FnOnce() -> Cell,
        F: FnOnce(&Cell) -> R,
    {
        let mut f_opt = Some(f);

        // 1. Optimistic read search
        let res = {
            let root_lock = self.root.read().expect("Lock poisoned");
            if let Some(ref node) = *root_lock {
                Self::find_recursive(node, coords, |c| {
                    if let Some(f) = f_opt.take() {
                        Some(f(c))
                    } else {
                        None
                    }
                })
                .flatten()
            } else {
                None
            }
        };

        if let Some(result) = res {
            return result;
        }

        // 2. Not found, acquire root write lock if tree is empty.
        let f = f_opt.take().expect("f already consumed in optimistic path");
        let mut root_lock = self.root.write().expect("Lock poisoned");
        if root_lock.is_none() {
            let cell = creator();
            let result = f(&cell);
            *root_lock = Some(Box::new(CellNode::new(cell)));
            return result;
        }

        // 3. Tree not empty, descend and insert with granular write locks
        let node = root_lock.as_ref().unwrap();
        Self::find_or_create_recursive(node, coords, creator, f)
    }

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

    pub fn collect_all_states(
        &self,
        current_mask: usize,
        last_mask: usize,
        out: &mut Vec<((i128, i128), u8)>,
    ) {
        let root_lock = self.root.read().expect("Lock poisoned");
        if let Some(ref node) = *root_lock {
            Self::collect_all_recursive(node, current_mask, last_mask, out);
        }
    }

    fn collect_all_recursive(
        node: &CellNode,
        current_mask: usize,
        last_mask: usize,
        out: &mut Vec<((i128, i128), u8)>,
    ) {
        out.push((
            node.cell.coordinates(),
            node.cell.presenter_view(current_mask, last_mask),
        ));

        {
            let left_lock = node.left.read().expect("Lock poisoned");
            if let Some(ref left_node) = *left_lock {
                Self::collect_all_recursive(left_node, current_mask, last_mask, out);
            }
        }
        {
            let right_lock = node.right.read().expect("Lock poisoned");
            if let Some(ref right_node) = *right_lock {
                Self::collect_all_recursive(right_node, current_mask, last_mask, out);
            }
        }
    }

    pub fn collect_in_rect(
        &self,
        min: (i128, i128),
        max: (i128, i128),
        current_mask: usize,
        last_mask: usize,
        out: &mut Vec<((i128, i128), u8)>,
    ) {
        let root_lock = self.root.read().expect("Lock poisoned");
        if let Some(ref node) = *root_lock {
            Self::collect_in_rect_recursive(node, min, max, current_mask, last_mask, out);
        }
    }

    fn collect_in_rect_recursive(
        node: &CellNode,
        min: (i128, i128),
        max: (i128, i128),
        current_mask: usize,
        last_mask: usize,
        out: &mut Vec<((i128, i128), u8)>,
    ) {
        let coords = node.cell.coordinates();

        // Check if in rect
        if coords.0 >= min.0 && coords.0 <= max.0 && coords.1 >= min.1 && coords.1 <= max.1 {
            out.push((coords, node.cell.presenter_view(current_mask, last_mask)));
        }

        // Lexicographical pruning:
        if Self::compare_coords(coords, min) == Ordering::Greater {
            let left_lock = node.left.read().expect("Lock poisoned");
            if let Some(ref left_node) = *left_lock {
                Self::collect_in_rect_recursive(left_node, min, max, current_mask, last_mask, out);
            }
        }

        if Self::compare_coords(coords, max) == Ordering::Less {
            let right_lock = node.right.read().expect("Lock poisoned");
            if let Some(ref right_node) = *right_lock {
                Self::collect_in_rect_recursive(right_node, min, max, current_mask, last_mask, out);
            }
        }
    }

    fn compare_coords(a: (i128, i128), b: (i128, i128)) -> Ordering {
        if a.0 != b.0 {
            a.0.cmp(&b.0)
        } else {
            a.1.cmp(&b.1)
        }
    }
}

#[derive(Copy, Clone)]
pub struct SendUnitPtr(pub *const CellNode);
unsafe impl Send for SendUnitPtr {}
unsafe impl Sync for SendUnitPtr {}

#[derive(Copy, Clone)]
pub struct SendUnitMutPtr(pub *mut CellNode);
unsafe impl Send for SendUnitMutPtr {}
unsafe impl Sync for SendUnitMutPtr {}

/// A thread-safe wrapper for raw pointers to RwLock<Option<Box<CellNode>>>.
#[derive(Copy, Clone)]
pub struct SendLockUnitPtr(pub *const RwLock<Option<Box<CellNode>>>);
unsafe impl Send for SendLockUnitPtr {}
unsafe impl Sync for SendLockUnitPtr {}
