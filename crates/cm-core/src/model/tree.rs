//! `src/model/chunk.js` (`LeafChunk`, `BranchChunk`) and the tree walks from
//! `src/line/utils_line.js` (`getLine`, `lineNo`, `updateLineHeight`,
//! `lineAtHeight`), as an index arena. Node 0 is the document root (in the JS the
//! `Doc` object *is* the root `BranchChunk`).
//!
//! Thresholds are the JS ones: leaves split above 50 lines into 25-line leaves,
//! branches spill above 10 children, and a branch collapses to one leaf below 25
//! lines.

use std::rc::Rc;

use super::line::{LineRef, clean_up_line};

pub(crate) type NodeId = usize;

pub(crate) enum Node {
    Leaf {
        lines: Vec<LineRef>,
        height: f64,
        parent: Option<NodeId>,
    },
    Branch {
        children: Vec<NodeId>,
        size: usize,
        height: f64,
        parent: Option<NodeId>,
    },
    Free,
}

pub(crate) struct Tree {
    nodes: Vec<Node>,
    free: Vec<NodeId>,
    /// Lines removed by `remove_inner` that have `delete` listeners, for the caller
    /// to signal once the document borrow is released.
    pub deleted_with_handlers: Vec<LineRef>,
}

pub(crate) const ROOT: NodeId = 0;

impl Tree {
    /// A root branch holding one leaf with the given lines.
    pub fn new(lines: Vec<LineRef>) -> Tree {
        let mut t = Tree {
            nodes: Vec::new(),
            free: Vec::new(),
            deleted_with_handlers: Vec::new(),
        };
        // Reserve the root slot first so it gets id 0.
        t.nodes.push(Node::Free);
        let leaf = t.new_leaf(lines);
        let (size, height) = (t.chunk_size(leaf), t.height(leaf));
        t.nodes[ROOT] = Node::Branch {
            children: vec![leaf],
            size,
            height,
            parent: None,
        };
        t.set_parent(leaf, Some(ROOT));
        t
    }

    fn alloc(&mut self, n: Node) -> NodeId {
        if let Some(id) = self.free.pop() {
            self.nodes[id] = n;
            id
        } else {
            self.nodes.push(n);
            self.nodes.len() - 1
        }
    }

    fn free_subtree(&mut self, id: NodeId) {
        if let Node::Branch { children, .. } = &self.nodes[id] {
            for c in children.clone() {
                self.free_subtree(c);
            }
        }
        self.nodes[id] = Node::Free;
        self.free.push(id);
    }

    /// `new LeafChunk(lines)`.
    fn new_leaf(&mut self, lines: Vec<LineRef>) -> NodeId {
        let height = lines.iter().map(|l| l.borrow().height).sum();
        let id = self.alloc(Node::Leaf {
            lines: Vec::new(),
            height,
            parent: None,
        });
        for l in &lines {
            l.borrow_mut().parent = Some(id);
        }
        if let Node::Leaf { lines: ls, .. } = &mut self.nodes[id] {
            *ls = lines;
        }
        id
    }

    /// `new BranchChunk(children)`.
    fn new_branch(&mut self, children: Vec<NodeId>) -> NodeId {
        let size = children.iter().map(|&c| self.chunk_size(c)).sum();
        let height = children.iter().map(|&c| self.height(c)).sum();
        let id = self.alloc(Node::Branch {
            children: children.clone(),
            size,
            height,
            parent: None,
        });
        for c in children {
            self.set_parent(c, Some(id));
        }
        id
    }

    pub fn chunk_size(&self, id: NodeId) -> usize {
        match &self.nodes[id] {
            Node::Leaf { lines, .. } => lines.len(),
            Node::Branch { size, .. } => *size,
            Node::Free => panic!("freed chunk"),
        }
    }

    pub fn height(&self, id: NodeId) -> f64 {
        match &self.nodes[id] {
            Node::Leaf { height, .. } | Node::Branch { height, .. } => *height,
            Node::Free => panic!("freed chunk"),
        }
    }

    fn height_mut(&mut self, id: NodeId) -> &mut f64 {
        match &mut self.nodes[id] {
            Node::Leaf { height, .. } | Node::Branch { height, .. } => height,
            Node::Free => panic!("freed chunk"),
        }
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        match &self.nodes[id] {
            Node::Leaf { parent, .. } | Node::Branch { parent, .. } => *parent,
            Node::Free => panic!("freed chunk"),
        }
    }

    fn set_parent(&mut self, id: NodeId, p: Option<NodeId>) {
        match &mut self.nodes[id] {
            Node::Leaf { parent, .. } | Node::Branch { parent, .. } => *parent = p,
            Node::Free => panic!("freed chunk"),
        }
    }

    fn children(&self, id: NodeId) -> &Vec<NodeId> {
        match &self.nodes[id] {
            Node::Branch { children, .. } => children,
            _ => panic!("not a branch"),
        }
    }

    fn children_mut(&mut self, id: NodeId) -> &mut Vec<NodeId> {
        match &mut self.nodes[id] {
            Node::Branch { children, .. } => children,
            _ => panic!("not a branch"),
        }
    }

    fn is_leaf(&self, id: NodeId) -> bool {
        matches!(self.nodes[id], Node::Leaf { .. })
    }

    pub fn size(&self) -> usize {
        self.chunk_size(ROOT)
    }

    /// `removeInner(at, n)` on any chunk. `at` is signed because `Doc.remove` can
    /// be handed a position before `doc.first` (see `updateDoc` for full changes);
    /// a leaf then reads `lines[negative]` and the JS throws.
    pub fn remove_inner(&mut self, id: NodeId, mut at: i64, mut n: usize) {
        if self.is_leaf(id) {
            if n == 0 {
                return;
            }
            let removed: Vec<LineRef> = match &mut self.nodes[id] {
                Node::Leaf { lines, height, .. } => {
                    if at < 0 || at as usize + n > lines.len() {
                        panic!(
                            "JS TypeError: Cannot read properties of undefined (reading 'height')"
                        );
                    }
                    let at = at as usize;
                    for l in &lines[at..at + n] {
                        *height -= l.borrow().height;
                    }
                    lines.drain(at..at + n).collect()
                }
                _ => unreachable!(),
            };
            for l in &removed {
                clean_up_line(l);
                if super::line::has_delete_handlers(l) {
                    self.deleted_with_handlers.push(l.clone());
                }
            }
            return;
        }
        match &mut self.nodes[id] {
            Node::Branch { size, .. } => *size -= n,
            _ => unreachable!(),
        }
        let mut i = 0;
        while i < self.children(id).len() {
            let child = self.children(id)[i];
            let sz = self.chunk_size(child) as i64;
            if at < sz {
                let rm = (n as i64).min(sz - at) as usize;
                let old_height = self.height(child);
                self.remove_inner(child, at, rm);
                let new_height = self.height(child);
                *self.height_mut(id) -= old_height - new_height;
                if sz == rm as i64 {
                    self.children_mut(id).remove(i);
                    self.set_parent(child, None);
                    self.free_subtree(child);
                    // `splice(i--, 1)` followed by the loop's `++i`.
                    n -= rm;
                    if n == 0 {
                        break;
                    }
                    at = 0;
                    continue;
                }
                n -= rm;
                if n == 0 {
                    break;
                }
                at = 0;
            } else {
                at -= sz;
            }
            i += 1;
        }
        // If the result is smaller than 25 lines, ensure that it is a single leaf node.
        let size = self.chunk_size(id);
        let children = self.children(id).clone();
        if size - n < 25
            && (children.len() > 1 || children.first().is_none_or(|&c| !self.is_leaf(c)))
        {
            let mut lines = Vec::new();
            self.collapse(id, &mut lines);
            for c in children {
                self.free_subtree(c);
            }
            let leaf = self.new_leaf(lines);
            *self.children_mut(id) = vec![leaf];
            self.set_parent(leaf, Some(id));
        }
    }

    /// `collapse(lines)`: gather every line below `id`, in order.
    fn collapse(&self, id: NodeId, out: &mut Vec<LineRef>) {
        match &self.nodes[id] {
            Node::Leaf { lines, .. } => out.extend(lines.iter().cloned()),
            Node::Branch { children, .. } => {
                for &c in children {
                    self.collapse(c, out);
                }
            }
            Node::Free => panic!("freed chunk"),
        }
    }

    /// `insertInner(at, lines, height)`. A negative `at` (see [`Tree::remove_inner`])
    /// descends into the first child and becomes a negative `slice` index in the
    /// leaf, i.e. counts from the end of that leaf.
    pub fn insert_inner(&mut self, id: NodeId, at: i64, new_lines: Vec<LineRef>, height: f64) {
        if self.is_leaf(id) {
            for l in &new_lines {
                l.borrow_mut().parent = Some(id);
            }
            if let Node::Leaf {
                lines, height: h, ..
            } = &mut self.nodes[id]
            {
                *h += height;
                let len = lines.len() as i64;
                let at = if at < 0 {
                    (len + at).max(0)
                } else {
                    at.min(len)
                } as usize;
                let tail = lines.split_off(at);
                lines.extend(new_lines);
                lines.extend(tail);
            }
            return;
        }
        let count = new_lines.len();
        if let Node::Branch {
            size, height: h, ..
        } = &mut self.nodes[id]
        {
            *size += count;
            *h += height;
        }
        let mut at = at;
        let mut i = 0;
        while i < self.children(id).len() {
            let child = self.children(id)[i];
            let sz = self.chunk_size(child) as i64;
            if at <= sz {
                self.insert_inner(child, at, new_lines, height);
                let child_len = match &self.nodes[child] {
                    Node::Leaf { lines, .. } => Some(lines.len()),
                    _ => None,
                };
                if let Some(len) = child_len.filter(|&l| l > 50) {
                    // Never splice a huge leaf; take 25-line slices from it in order.
                    let remaining = len % 25 + 25;
                    let all: Vec<LineRef> = match &self.nodes[child] {
                        Node::Leaf { lines, .. } => lines.clone(),
                        _ => unreachable!(),
                    };
                    let mut pos = remaining;
                    while pos < len {
                        let end = (pos + 25).min(len);
                        let leaf = self.new_leaf(all[pos..end].to_vec());
                        pos += 25;
                        let lh = self.height(leaf);
                        *self.height_mut(child) -= lh;
                        i += 1;
                        self.children_mut(id).insert(i, leaf);
                        self.set_parent(leaf, Some(id));
                    }
                    if let Node::Leaf { lines, .. } = &mut self.nodes[child] {
                        lines.truncate(remaining);
                    }
                    self.maybe_spill(id);
                }
                return;
            }
            at -= sz;
            i += 1;
        }
    }

    /// `maybeSpill()`: split a branch that grew beyond 10 children.
    fn maybe_spill(&mut self, id: NodeId) {
        if self.children(id).len() <= 10 {
            return;
        }
        let mut me = id;
        loop {
            let len = self.children(me).len();
            let spilled: Vec<NodeId> = self.children_mut(me).drain(len - 5..).collect();
            let sibling = self.new_branch(spilled);
            match self.parent(me) {
                None => {
                    // Become the parent node.
                    let kids = std::mem::take(self.children_mut(me));
                    let copy = self.new_branch(kids);
                    self.set_parent(copy, Some(me));
                    *self.children_mut(me) = vec![copy, sibling];
                    me = copy;
                }
                Some(p) => {
                    let (ss, sh) = (self.chunk_size(sibling), self.height(sibling));
                    if let Node::Branch { size, height, .. } = &mut self.nodes[me] {
                        *size -= ss;
                        *height -= sh;
                    }
                    let idx = self
                        .children(p)
                        .iter()
                        .position(|&c| c == me)
                        .expect("child in parent");
                    self.children_mut(p).insert(idx + 1, sibling);
                }
            }
            let mp = self.parent(me);
            self.set_parent(sibling, mp);
            if self.children(me).len() <= 10 {
                break;
            }
        }
        let p = self.parent(me).expect("spilled chunk has a parent");
        self.maybe_spill(p);
    }

    /// Lines `at .. at + n` (the `iterN` walk, collected).
    pub fn lines_range(&self, at: usize, n: usize) -> Vec<LineRef> {
        let mut out = Vec::with_capacity(n.min(self.size()));
        self.collect_range(ROOT, at, n, &mut out);
        out
    }

    fn collect_range(&self, id: NodeId, mut at: usize, mut n: usize, out: &mut Vec<LineRef>) {
        match &self.nodes[id] {
            Node::Leaf { lines, .. } => {
                out.extend(lines[at..(at + n).min(lines.len())].iter().cloned())
            }
            Node::Branch { children, .. } => {
                for &c in children {
                    let sz = self.chunk_size(c);
                    if at < sz {
                        let used = n.min(sz - at);
                        self.collect_range(c, at, used, out);
                        n -= used;
                        if n == 0 {
                            break;
                        }
                        at = 0;
                    } else {
                        at -= sz;
                    }
                }
            }
            Node::Free => panic!("freed chunk"),
        }
    }

    /// `getLine` relative to the first line (`n -= doc.first` already applied).
    pub fn get(&self, mut n: usize) -> LineRef {
        let mut id = ROOT;
        loop {
            match &self.nodes[id] {
                Node::Leaf { lines, .. } => return lines[n].clone(),
                Node::Branch { children, .. } => {
                    let mut next = None;
                    for &c in children {
                        let sz = self.chunk_size(c);
                        if n < sz {
                            next = Some(c);
                            break;
                        }
                        n -= sz;
                    }
                    id = next.expect("line index within chunk");
                }
                Node::Free => panic!("freed chunk"),
            }
        }
    }

    /// `lineNo(line) - doc.first`, or `None` for a detached line.
    pub fn index_of_line(&self, line: &LineRef) -> Option<usize> {
        let leaf = line.borrow().parent?;
        let mut no = match &self.nodes[leaf] {
            // `indexOf` returns -1 for a missing line; that cannot happen for an
            // attached line, so treat it as detached.
            Node::Leaf { lines, .. } => lines.iter().position(|l| Rc::ptr_eq(l, line))?,
            _ => return None,
        };
        let mut cur = leaf;
        while let Some(p) = self.parent(cur) {
            for &c in self.children(p) {
                if c == cur {
                    break;
                }
                no += self.chunk_size(c);
            }
            cur = p;
        }
        if cur != ROOT {
            return None;
        }
        Some(no)
    }

    /// `updateLineHeight(line, height)`.
    pub fn update_line_height(&mut self, line: &LineRef, height: f64) {
        let diff = height - line.borrow().height;
        if diff != 0.0 {
            line.borrow_mut().height += diff;
            let mut n = line.borrow().parent;
            while let Some(id) = n {
                *self.height_mut(id) += diff;
                n = self.parent(id);
            }
        }
    }

    /// Height above a line (`heightAtLine`, minus the `visualLine` step).
    pub fn height_above(&self, line: &LineRef) -> f64 {
        let mut h = 0.0;
        let Some(leaf) = line.borrow().parent else {
            return 0.0;
        };
        if let Node::Leaf { lines, .. } = &self.nodes[leaf] {
            for l in lines {
                if Rc::ptr_eq(l, line) {
                    break;
                }
                h += l.borrow().height;
            }
        }
        let mut chunk = leaf;
        while let Some(p) = self.parent(chunk) {
            for &c in self.children(p) {
                if c == chunk {
                    break;
                }
                h += self.height(c);
            }
            chunk = p;
        }
        h
    }

    /// `lineAtHeight(chunk, h)` from the root; returns an offset from `doc.first`.
    pub fn line_at_height(&self, mut h: f64) -> usize {
        let mut n = 0;
        let mut id = ROOT;
        'outer: loop {
            match &self.nodes[id] {
                Node::Branch { children, .. } => {
                    for &c in children {
                        let ch = self.height(c);
                        if h < ch {
                            id = c;
                            continue 'outer;
                        }
                        h -= ch;
                        n += self.chunk_size(c);
                    }
                    return n;
                }
                Node::Leaf { lines, .. } => {
                    let mut i = 0;
                    while i < lines.len() {
                        let lh = lines[i].borrow().height;
                        if h < lh {
                            break;
                        }
                        h -= lh;
                        i += 1;
                    }
                    return n + i;
                }
                Node::Free => panic!("freed chunk"),
            }
        }
    }

    // Ported for completeness; only the (unported) view layer calls it.
    #[allow(dead_code)]
    pub fn total_height(&self) -> f64 {
        self.height(ROOT)
    }

    /// Debug aid: check sizes, heights and parent links.
    #[doc(hidden)]
    pub fn check_invariants(&self) -> Result<(), String> {
        self.check(ROOT, None).map(|_| ())
    }

    fn check(&self, id: NodeId, parent: Option<NodeId>) -> Result<(usize, f64), String> {
        if self.parent(id) != parent {
            return Err(format!("bad parent link at {id}"));
        }
        match &self.nodes[id] {
            Node::Leaf { lines, height, .. } => {
                let mut h = 0.0;
                for l in lines {
                    if l.borrow().parent != Some(id) {
                        return Err(format!("bad line parent in leaf {id}"));
                    }
                    h += l.borrow().height;
                }
                if h != *height {
                    return Err(format!("bad leaf height {id}"));
                }
                Ok((lines.len(), h))
            }
            Node::Branch {
                children,
                size,
                height,
                ..
            } => {
                let (mut s, mut h) = (0, 0.0);
                for &c in children {
                    let (cs, chh) = self.check(c, Some(id))?;
                    s += cs;
                    h += chh;
                }
                if s != *size || h != *height {
                    return Err(format!("bad branch totals {id}"));
                }
                Ok((s, h))
            }
            Node::Free => Err("free node reachable".into()),
        }
    }

    /// Depth of the tree (for tests that want to see the B-tree actually branch).
    #[doc(hidden)]
    pub fn depth(&self) -> usize {
        fn d(t: &Tree, id: NodeId) -> usize {
            match &t.nodes[id] {
                Node::Branch { children, .. } => {
                    1 + children.iter().map(|&c| d(t, c)).max().unwrap_or(0)
                }
                _ => 1,
            }
        }
        d(self, ROOT)
    }
}
