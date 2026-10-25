//! `src/model/selection.js`: `Selection`, `Range`, `normalizeSelection`,
//! `simpleSelection`.
//!
//! Internally a selection is an `Rc` (the JS shares selection objects between
//! `doc.sel`, the history arrays and copied docs) whose ranges hold [`PosRef`]s.
//! The public API exposes plain value snapshots ([`Selection`], [`SelRange`]).

use std::cell::Cell;
use std::cmp::Ordering;
use std::rc::Rc;

use crate::pos::{Pos, PosRef, equal_cursor_pos, max_pos, min_pos, rcmp};

#[derive(Clone, Debug)]
pub(crate) struct Range {
    pub anchor: PosRef,
    pub head: PosRef,
    /// `range.goalColumn`, an expando property `moveV` (src/edit/methods.js) puts on
    /// the JS `Range` object. Shared between clones, as JS object identity is.
    pub goal_column: Rc<Cell<Option<f64>>>,
}

impl Range {
    pub fn new(anchor: PosRef, head: PosRef) -> Range {
        Range {
            anchor,
            head,
            goal_column: Rc::new(Cell::new(None)),
        }
    }
    pub fn from(&self) -> PosRef {
        min_pos(&self.anchor, &self.head)
    }
    pub fn to(&self) -> PosRef {
        max_pos(&self.anchor, &self.head)
    }
    pub fn empty(&self) -> bool {
        let (a, h) = (self.anchor.get(), self.head.get());
        h.line == a.line && h.ch == a.ch
    }
}

#[derive(Debug)]
pub(crate) struct SelectionData {
    pub ranges: Vec<Range>,
    pub prim_index: usize,
    /// Set on history copies by `rebaseHistArray` (copy-on-write marker).
    pub copied: Cell<bool>,
}

pub(crate) type Sel = Rc<SelectionData>;

pub(crate) fn new_sel(ranges: Vec<Range>, prim_index: usize) -> Sel {
    Rc::new(SelectionData {
        ranges,
        prim_index,
        copied: Cell::new(false),
    })
}

impl SelectionData {
    pub fn primary(&self) -> &Range {
        &self.ranges[self.prim_index]
    }

    pub fn deep_copy(&self) -> SelectionData {
        let ranges = self
            .ranges
            .iter()
            .map(|r| {
                Range::new(
                    PosRef::lc(r.anchor.line(), r.anchor.ch()),
                    PosRef::lc(r.head.line(), r.head.ch()),
                )
            })
            .collect();
        SelectionData {
            ranges,
            prim_index: self.prim_index,
            copied: Cell::new(false),
        }
    }

    pub fn something_selected(&self) -> bool {
        self.ranges.iter().any(|r| !r.empty())
    }

    /// `contains(pos, end)`: index of the first range touching `pos..end`, or -1.
    // Ported for completeness; only the (unported) view layer calls it.
    #[allow(dead_code)]
    pub fn contains(&self, pos: &Pos, end: Option<&Pos>) -> i64 {
        let end = end.unwrap_or(pos);
        for (i, r) in self.ranges.iter().enumerate() {
            if crate::pos::cmp(end, &r.from().get()) != Ordering::Less
                && crate::pos::cmp(pos, &r.to().get()) != Ordering::Greater
            {
                return i as i64;
            }
        }
        -1
    }

    pub fn snapshot(&self) -> Selection {
        Selection {
            ranges: self
                .ranges
                .iter()
                .map(|r| SelRange {
                    anchor: r.anchor.get(),
                    head: r.head.get(),
                })
                .collect(),
            primary: self.prim_index,
        }
    }
}

/// `Selection.prototype.equals`.
pub(crate) fn sel_equals(a: &Sel, b: &Sel) -> bool {
    if Rc::ptr_eq(a, b) {
        return true;
    }
    if a.prim_index != b.prim_index || a.ranges.len() != b.ranges.len() {
        return false;
    }
    a.ranges.iter().zip(b.ranges.iter()).all(|(here, there)| {
        equal_cursor_pos(&here.anchor.get(), &there.anchor.get())
            && equal_cursor_pos(&here.head.get(), &there.head.get())
    })
}

/// `normalizeSelection(cm, ranges, primIndex)`: sort, merge overlapping ranges, keep
/// track of the primary one.
pub(crate) fn normalize_selection(may_touch: bool, ranges: Vec<Range>, prim_index: usize) -> Sel {
    // Array.prototype.sort is stable (V8 TimSort), as is sort_by.
    let mut tagged: Vec<(Range, usize)> = ranges
        .into_iter()
        .enumerate()
        .map(|(i, r)| (r, i))
        .collect();
    tagged.sort_by(|a, b| rcmp(&a.0.from(), &b.0.from()));
    let mut prim = tagged
        .iter()
        .position(|(_, i)| *i == prim_index)
        .map(|p| p as i64)
        .unwrap_or(-1);
    let mut ranges: Vec<Range> = tagged.into_iter().map(|(r, _)| r).collect();
    let mut i = 1;
    while i < ranges.len() {
        let cur = ranges[i].clone();
        let prev = ranges[i - 1].clone();
        let diff = crate::pos::cmp_num(&prev.to().get(), &cur.from().get());
        let merge = if may_touch && !cur.empty() {
            diff > 0
        } else {
            diff >= 0
        };
        if merge {
            let from = min_pos(&prev.from(), &cur.from());
            let to = max_pos(&prev.to(), &cur.to());
            // Identity comparisons in the JS (`cur.from() == cur.head`).
            let inv = if prev.empty() {
                PosRef::same(&cur.from(), &cur.head)
            } else {
                PosRef::same(&prev.from(), &prev.head)
            };
            if (i as i64) <= prim {
                prim -= 1;
            }
            i -= 1;
            ranges.splice(
                i..i + 2,
                [Range::new(
                    if inv { to.clone() } else { from.clone() },
                    if inv { from } else { to },
                )],
            );
        }
        i += 1;
    }
    // A primary index of -1 would make `primary()` undefined in the JS; it cannot
    // happen for in-range indices.
    new_sel(ranges, prim.max(0) as usize)
}

/// `simpleSelection(anchor, head)`; pass the same `PosRef` twice for the JS
/// `head || anchor` case.
pub(crate) fn simple_selection(anchor: PosRef, head: Option<PosRef>) -> Sel {
    let head = head.unwrap_or_else(|| anchor.clone());
    new_sel(vec![Range::new(anchor, head)], 0)
}

/// A selection range snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelRange {
    pub anchor: Pos,
    pub head: Pos,
}

impl SelRange {
    pub fn new(anchor: Pos, head: Pos) -> SelRange {
        SelRange { anchor, head }
    }
    pub fn cursor(p: Pos) -> SelRange {
        SelRange { anchor: p, head: p }
    }
    pub fn from(&self) -> Pos {
        if crate::pos::cmp(&self.anchor, &self.head) == Ordering::Less {
            self.anchor
        } else {
            self.head
        }
    }
    pub fn to(&self) -> Pos {
        if crate::pos::cmp(&self.anchor, &self.head) == Ordering::Less {
            self.head
        } else {
            self.anchor
        }
    }
    pub fn empty(&self) -> bool {
        self.anchor.line == self.head.line && self.anchor.ch == self.head.ch
    }
}

/// A selection snapshot: the ranges and the primary index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub ranges: Vec<SelRange>,
    pub primary: usize,
}

impl Selection {
    pub fn primary(&self) -> SelRange {
        self.ranges[self.primary]
    }
}
