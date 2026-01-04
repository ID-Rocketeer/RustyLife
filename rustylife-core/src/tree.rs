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

    pub fn collect_living(&self, current_idx: usize, out: &mut Vec<(i128, i128)>) {
        let root_lock = self.root.read().expect("Lock poisoned");
        if let Some(ref node) = *root_lock {
            Self::collect_recursive(node, current_idx, out);
        }
    }

    fn collect_recursive(node: &CellNode, current_idx: usize, out: &mut Vec<(i128, i128)>) {
        if node.cell.state(current_idx) == crate::cell::CellState::Alive {
            out.push(node.cell.coordinates());
        }

        {
            let left_lock = node.left.read().expect("Lock poisoned");
            if let Some(ref left_node) = *left_lock {
                Self::collect_recursive(left_node, current_idx, out);
            }
        }
        {
            let right_lock = node.right.read().expect("Lock poisoned");
            if let Some(ref right_node) = *right_lock {
                Self::collect_recursive(right_node, current_idx, out);
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
pub struct SendNodePtr(pub *const CellNode);
unsafe impl Send for SendNodePtr {}
unsafe impl Sync for SendNodePtr {}

#[derive(Copy, Clone)]
pub struct SendNodeMutPtr(pub *mut CellNode);
unsafe impl Send for SendNodeMutPtr {}
unsafe impl Sync for SendNodeMutPtr {}

/// A thread-safe wrapper for raw pointers to RwLock<Option<Box<CellNode>>>.
#[derive(Copy, Clone)]
pub struct SendLockPtr(pub *const RwLock<Option<Box<CellNode>>>);
unsafe impl Send for SendLockPtr {}
unsafe impl Sync for SendLockPtr {}
