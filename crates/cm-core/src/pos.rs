//! `src/line/pos.js`.
//!
//! Public positions are plain values ([`Pos`]). Internally the model needs JavaScript
//! *object identity* for positions: CM5 compares positions with `==` in a few places
//! (`skipAtomicInSelection`, `normalizeSelection`) and mutates them in place when
//! rebasing history (`rebaseHistSelSingle`), which leaks into every selection that
//! shares the object. [`PosRef`] reproduces that.

use std::cell::Cell;
use std::cmp::Ordering;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sticky {
    Before,
    After,
}

impl Sticky {
    pub fn as_str(self) -> &'static str {
        match self {
            Sticky::Before => "before",
            Sticky::After => "after",
        }
    }
}

/// A `{line, ch, sticky}` position. `ch` counts UTF-16 code units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pos {
    pub line: i64,
    pub ch: i64,
    pub sticky: Option<Sticky>,
}

/// Stand-in for an omitted `ch` (`Pos(line)` in JS, meaning end of line). Clipping
/// treats it exactly like `null`.
pub const CH_END: i64 = i64::MAX;

impl Pos {
    pub const fn new(line: i64, ch: i64) -> Pos {
        Pos {
            line,
            ch,
            sticky: None,
        }
    }
    pub const fn with_sticky(line: i64, ch: i64, sticky: Option<Sticky>) -> Pos {
        Pos { line, ch, sticky }
    }
    /// `Pos(line)` — end of the line once clipped.
    pub const fn line_end(line: i64) -> Pos {
        Pos {
            line,
            ch: CH_END,
            sticky: None,
        }
    }
}

/// `cmp(a, b)`: `a.line - b.line || a.ch - b.ch`.
pub fn cmp(a: &Pos, b: &Pos) -> Ordering {
    a.line.cmp(&b.line).then(a.ch.cmp(&b.ch))
}

/// `cmp` as the JS number (sign only matters, but some callers use the value).
pub fn cmp_num(a: &Pos, b: &Pos) -> i64 {
    let d = a.line - b.line;
    if d != 0 { d } else { a.ch - b.ch }
}

pub fn equal_cursor_pos(a: &Pos, b: &Pos) -> bool {
    a.sticky == b.sticky && cmp(a, b) == Ordering::Equal
}

#[derive(Debug)]
pub(crate) struct PosCell {
    line: Cell<i64>,
    ch: Cell<i64>,
    sticky: Cell<Option<Sticky>>,
}

/// A position *object* with JS reference semantics.
#[derive(Clone, Debug)]
pub(crate) struct PosRef(Rc<PosCell>);

impl PosRef {
    pub fn new(p: Pos) -> PosRef {
        PosRef(Rc::new(PosCell {
            line: Cell::new(p.line),
            ch: Cell::new(p.ch),
            sticky: Cell::new(p.sticky),
        }))
    }
    pub fn lc(line: i64, ch: i64) -> PosRef {
        PosRef::new(Pos::new(line, ch))
    }
    pub fn get(&self) -> Pos {
        Pos {
            line: self.0.line.get(),
            ch: self.0.ch.get(),
            sticky: self.0.sticky.get(),
        }
    }
    pub fn line(&self) -> i64 {
        self.0.line.get()
    }
    pub fn ch(&self) -> i64 {
        self.0.ch.get()
    }
    pub fn set_line(&self, l: i64) {
        self.0.line.set(l)
    }
    pub fn set_ch(&self, c: i64) {
        self.0.ch.set(c)
    }
    pub fn same(a: &PosRef, b: &PosRef) -> bool {
        Rc::ptr_eq(&a.0, &b.0)
    }
}

pub(crate) fn rcmp(a: &PosRef, b: &PosRef) -> Ordering {
    cmp(&a.get(), &b.get())
}

/// `minPos(a, b)`: `cmp(a, b) < 0 ? a : b` (returns one of the objects).
pub(crate) fn min_pos(a: &PosRef, b: &PosRef) -> PosRef {
    if rcmp(a, b) == Ordering::Less {
        a.clone()
    } else {
        b.clone()
    }
}

/// `maxPos(a, b)`: `cmp(a, b) < 0 ? b : a`.
pub(crate) fn max_pos(a: &PosRef, b: &PosRef) -> PosRef {
    if rcmp(a, b) == Ordering::Less {
        b.clone()
    } else {
        a.clone()
    }
}
