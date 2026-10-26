//! `src/model/selection_updates.js`: setting the selection, history recording of
//! selections, and keeping cursors out of atomic markers.

use std::cmp::Ordering;

use super::changes::BeforeSelectionChange;
use super::doc::Doc;
use super::history::{HistEvent, SelHistOpts, add_selection_to_history};
use super::mark_text::{SideFind, TextMarker};
use super::selection::{
    Range, Sel, SelRange, new_sel, normalize_selection, sel_equals, simple_selection,
};
use crate::pos::{Pos, PosRef, cmp, cmp_num};

/// Selection update options (`{origin, bias, scroll, clearRedo}`). Whether options
/// are passed *at all* matters in the JS (`options && options.clearRedo !== false`),
/// so APIs take `Option<SelOptions>`.
#[derive(Clone, Debug, Default)]
pub struct SelOptions {
    pub origin: Option<String>,
    pub bias: Option<i64>,
    pub scroll: Option<bool>,
    pub clear_redo: Option<bool>,
}

/// `extendRange(range, head, other, extend)`.
pub(crate) fn extend_range(
    range: &Range,
    head: PosRef,
    other: Option<PosRef>,
    extend: bool,
) -> Range {
    if extend {
        let mut anchor = range.anchor.clone();
        let mut head = head;
        if let Some(other) = other {
            let pos_before = cmp(&head.get(), &anchor.get()) == Ordering::Less;
            if pos_before != (cmp(&other.get(), &anchor.get()) == Ordering::Less) {
                anchor = head;
                head = other;
            } else if pos_before != (cmp(&head.get(), &other.get()) == Ordering::Less) {
                head = other;
            }
        }
        Range::new(anchor, head)
    } else {
        Range::new(other.unwrap_or_else(|| head.clone()), head)
    }
}

/// `extendSelection(doc, head, other, options, extend)`.
pub(crate) fn extend_selection(
    doc: &Doc,
    head: PosRef,
    other: Option<PosRef>,
    options: Option<&SelOptions>,
    extend: Option<bool>,
) {
    let extend = extend.unwrap_or_else(|| doc.extend_flag());
    let prim = doc.inner().sel.primary().clone();
    set_selection(
        doc,
        new_sel(vec![extend_range(&prim, head, other, extend)], 0),
        options,
    );
}

/// `extendSelections(doc, heads, options)`.
pub(crate) fn extend_selections(doc: &Doc, heads: Vec<PosRef>, options: Option<&SelOptions>) {
    let extend = doc.extend_flag();
    let sel = doc.inner().sel.clone();
    let out: Vec<Range> = sel
        .ranges
        .iter()
        .zip(heads)
        .map(|(r, h)| extend_range(r, h, None, extend))
        .collect();
    let new_sel = normalize_selection(doc.selections_may_touch(), out, sel.prim_index);
    set_selection(doc, new_sel, options);
}

/// `replaceOneSelection(doc, i, range, options)`.
// Ported for completeness; only the (unported) view layer calls it.
#[allow(dead_code)]
pub(crate) fn replace_one_selection(
    doc: &Doc,
    i: usize,
    range: Range,
    options: Option<&SelOptions>,
) {
    let sel = doc.inner().sel.clone();
    let mut ranges = sel.ranges.clone();
    ranges[i] = range;
    set_selection(
        doc,
        normalize_selection(doc.selections_may_touch(), ranges, sel.prim_index),
        options,
    );
}

/// `setSimpleSelection(doc, anchor, head, options)`.
pub(crate) fn set_simple_selection(
    doc: &Doc,
    anchor: PosRef,
    head: Option<PosRef>,
    options: Option<&SelOptions>,
) {
    set_selection(doc, simple_selection(anchor, head), options);
}

/// `filterSelectionChange(doc, sel, options)`.
fn filter_selection_change(doc: &Doc, sel: Sel, options: Option<&SelOptions>) -> Sel {
    let handlers = doc.inner().before_selection_change.snapshot();
    let mut obj = BeforeSelectionChange::new(
        sel.ranges
            .iter()
            .map(|r| SelRange {
                anchor: r.anchor.get(),
                head: r.head.get(),
            })
            .collect(),
        options.and_then(|o| o.origin.clone()),
    );
    let mut updated: Option<Vec<Range>> = None;
    for h in handlers.iter() {
        (h.f)(doc, &mut obj);
        if let Some(rs) = obj.updated.take() {
            let clipped: Vec<Range> = rs
                .iter()
                .map(|r| {
                    Range::new(
                        PosRef::new(doc.clip_pos(r.anchor)),
                        PosRef::new(doc.clip_pos(r.head)),
                    )
                })
                .collect();
            obj.ranges = clipped
                .iter()
                .map(|r| SelRange {
                    anchor: r.anchor.get(),
                    head: r.head.get(),
                })
                .collect();
            updated = Some(clipped);
        }
    }
    match updated {
        Some(ranges) => {
            let n = ranges.len();
            normalize_selection(doc.selections_may_touch(), ranges, n.saturating_sub(1))
        }
        None => sel,
    }
}

/// `setSelectionReplaceHistory(doc, sel, options)`.
pub(crate) fn set_selection_replace_history(doc: &Doc, sel: Sel, options: Option<&SelOptions>) {
    let hist = doc.history_rc();
    let last_is_sel = hist.borrow().lst_done().is_some_and(|e| e.is_sel());
    if last_is_sel {
        hist.borrow_mut().set_last_done(HistEvent::Sel(sel.clone()));
        set_selection_no_undo(doc, sel, options);
    } else {
        set_selection(doc, sel, options);
    }
}

/// `setSelection(doc, sel, options)`: set and record in history.
pub(crate) fn set_selection(doc: &Doc, sel: Sel, options: Option<&SelOptions>) {
    set_selection_no_undo(doc, sel, options);
    let cur = doc.inner().sel.clone();
    add_selection_to_history(
        doc,
        cur,
        doc.op_id(),
        options.map(|o| SelHistOpts {
            origin: o.origin.as_deref(),
            clear_redo: o.clear_redo,
        }),
    );
}

/// `setSelectionNoUndo(doc, sel, options)`.
pub(crate) fn set_selection_no_undo(doc: &Doc, sel: Sel, options: Option<&SelOptions>) {
    let sel = if doc.inner().before_selection_change.is_empty() {
        sel
    } else {
        filter_selection_change(doc, sel, options)
    };
    let bias = match options.and_then(|o| o.bias) {
        Some(b) if b != 0 => b,
        _ => {
            let cur_head = doc.inner().sel.primary().head.get();
            if cmp(&sel.primary().head.get(), &cur_head) == Ordering::Less {
                -1
            } else {
                1
            }
        }
    };
    let skipped = skip_atomic_in_selection(doc, &sel, Some(bias), true);
    set_selection_inner(doc, skipped);
    // `if (!(options && options.scroll === false) && doc.cm) ensureCursorVisible(doc.cm)`
    if options.and_then(|o| o.scroll) != Some(false) && doc.inner().editor.is_some() {
        doc.request_cursor_visible();
    }
}

fn set_selection_inner(doc: &Doc, sel: Sel) {
    let same = sel_equals(&sel, &doc.inner().sel);
    if same {
        return;
    }
    doc.inner_mut().sel = sel;
}

/// `reCheckSelection(doc)`: make sure the selection does not partially select any
/// atomic ranges.
pub(crate) fn re_check_selection(doc: &Doc) {
    let sel = doc.inner().sel.clone();
    let s = skip_atomic_in_selection(doc, &sel, None, false);
    set_selection_inner(doc, s);
}

/// `skipAtomicInSelection(doc, sel, bias, mayClear)`. Uses position identity, like
/// the JS, to decide whether anything moved.
fn skip_atomic_in_selection(doc: &Doc, sel: &Sel, bias: Option<i64>, may_clear: bool) -> Sel {
    let mut out: Option<Vec<Range>> = None;
    for i in 0..sel.ranges.len() {
        let range = &sel.ranges[i];
        let cur = doc.inner().sel.clone();
        let old = if sel.ranges.len() == cur.ranges.len() {
            Some(cur.ranges[i].clone())
        } else {
            None
        };
        let new_anchor = skip_atomic(
            doc,
            &range.anchor,
            old.as_ref().map(|o| &o.anchor),
            bias,
            may_clear,
        );
        let new_head = if PosRef::same(&range.head, &range.anchor) {
            new_anchor.clone()
        } else {
            skip_atomic(
                doc,
                &range.head,
                old.as_ref().map(|o| &o.head),
                bias,
                may_clear,
            )
        };
        if out.is_some()
            || !PosRef::same(&new_anchor, &range.anchor)
            || !PosRef::same(&new_head, &range.head)
        {
            let o = out.get_or_insert_with(|| sel.ranges[..i].to_vec());
            o.push(Range::new(new_anchor, new_head));
        }
    }
    match out {
        Some(out) => normalize_selection(doc.selections_may_touch(), out, sel.prim_index),
        None => sel.clone(),
    }
}

fn prevent_cursor(m: &TextMarker) -> (bool, bool) {
    let d = m.data();
    let left = match d.opts.select_left {
        Some(s) => !s,
        None => d.opts.inclusive_left,
    };
    let right = match d.opts.select_right {
        Some(s) => !s,
        None => d.opts.inclusive_right,
    };
    (left, right)
}

thread_local! {
    static SKIP_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Recursion bound standing in for the JS engine's stack limit: atomic markers in a
/// corrupted document can make `skipAtomicInner` recurse forever, which V8 reports
/// as a RangeError. Legitimate chains are bounded by the number of markers.
const MAX_SKIP_DEPTH: usize = 1000;

fn skip_atomic_inner(
    doc: &Doc,
    pos: &PosRef,
    old_pos: Option<&PosRef>,
    dir: i64,
    may_clear: bool,
) -> Option<PosRef> {
    let depth = SKIP_DEPTH.with(|d| {
        d.set(d.get() + 1);
        d.get()
    });
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            SKIP_DEPTH.with(|d| d.set(d.get() - 1));
        }
    }
    let _guard = Guard;
    if depth > MAX_SKIP_DEPTH {
        panic!("Maximum call stack size exceeded");
    }
    skip_atomic_inner_body(doc, pos, old_pos, dir, may_clear)
}

fn skip_atomic_inner_body(
    doc: &Doc,
    pos: &PosRef,
    old_pos: Option<&PosRef>,
    dir: i64,
    may_clear: bool,
) -> Option<PosRef> {
    let line = doc.get_line_ref(pos.line());
    let mut i: i64 = 0;
    loop {
        let sp = {
            let l = line.borrow();
            match l.marked_spans.as_ref().and_then(|a| a.at(i as usize)) {
                Some(sp) => sp,
                None => break,
            }
        };
        let m = sp.marker.clone();
        // Historically inclusiveLeft/Right decided this; selectLeft/Right override it.
        let (prevent_left, prevent_right) = prevent_cursor(&m);
        let p = pos.get();
        let left_ok = match sp.from {
            None => true,
            Some(f) => {
                if prevent_left {
                    f <= p.ch
                } else {
                    f < p.ch
                }
            }
        };
        let right_ok = match sp.to {
            None => true,
            Some(t) => {
                if prevent_right {
                    t >= p.ch
                } else {
                    t > p.ch
                }
            }
        };
        if left_ok && right_ok {
            if may_clear {
                // signal(m, "beforeCursorEnter"): the only model-level listener is
                // the clearOnEnter one.
                if m.data().opts.clear_on_enter {
                    m.clear();
                }
                if m.data().explicitly_cleared {
                    if line.borrow().marked_spans.is_none() {
                        break;
                    } else {
                        // `--i; continue` followed by the loop's `++i`.
                        continue;
                    }
                }
            }
            if !m.data().atomic {
                i += 1;
                continue;
            }

            if let Some(old_pos) = old_pos {
                let near_found = m.find_side(if dir < 0 { 1 } else { -1 });
                let mut near: Option<PosRef> = match near_found {
                    SideFind::Pos(p) => Some(PosRef::new(p)),
                    // A `{from, to}` object: truthy, but `.line` is undefined.
                    SideFind::Object => {
                        if if dir < 0 { prevent_right } else { prevent_left } {
                            panic!(
                                "JS TypeError: Cannot read properties of undefined (reading 'chunkSize')"
                            );
                        }
                        None
                    }
                    SideFind::None => None,
                };
                if if dir < 0 { prevent_right } else { prevent_left } {
                    let n = near
                        .expect("JS TypeError: Cannot read properties of undefined (reading 'ch')");
                    let same_line = n.line() == p.line;
                    near = move_pos(doc, &n, -dir, if same_line { Some(&line) } else { None });
                }
                if let Some(near) = near {
                    if near.line() == p.line {
                        let diff = cmp_num(&near.get(), &old_pos.get());
                        if diff != 0 && (if dir < 0 { diff < 0 } else { diff > 0 }) {
                            return skip_atomic_inner(doc, &near, Some(pos), dir, may_clear);
                        }
                    }
                }
            }

            let mut far = match m.find_side(if dir < 0 { -1 } else { 1 }) {
                SideFind::Pos(p) => Some(PosRef::new(p)),
                // getLine(doc, undefined) walks the tree with NaN and throws.
                SideFind::Object => panic!(
                    "JS TypeError: Cannot read properties of undefined (reading 'chunkSize')"
                ),
                SideFind::None => None,
            };
            if if dir < 0 { prevent_left } else { prevent_right } {
                let f =
                    far.expect("JS TypeError: Cannot read properties of undefined (reading 'ch')");
                let same_line = f.line() == p.line;
                far = move_pos(doc, &f, dir, if same_line { Some(&line) } else { None });
            }
            return match far {
                Some(far) => skip_atomic_inner(doc, &far, Some(pos), dir, may_clear),
                None => None,
            };
        }
        i += 1;
    }
    Some(pos.clone())
}

/// `skipAtomic(doc, pos, oldPos, bias, mayClear)`: ensure a position is not inside
/// an atomic range. Returns the *same* object when nothing moved.
pub(crate) fn skip_atomic(
    doc: &Doc,
    pos: &PosRef,
    old_pos: Option<&PosRef>,
    bias: Option<i64>,
    may_clear: bool,
) -> PosRef {
    let dir = match bias {
        Some(b) if b != 0 => b,
        _ => 1,
    };
    let found = skip_atomic_inner(doc, pos, old_pos, dir, may_clear)
        .or_else(|| {
            if !may_clear {
                skip_atomic_inner(doc, pos, old_pos, dir, true)
            } else {
                None
            }
        })
        .or_else(|| skip_atomic_inner(doc, pos, old_pos, -dir, may_clear))
        .or_else(|| {
            if !may_clear {
                skip_atomic_inner(doc, pos, old_pos, -dir, true)
            } else {
                None
            }
        });
    match found {
        Some(f) => f,
        None => {
            doc.inner_mut().cant_edit = true;
            PosRef::lc(doc.first_line(), 0)
        }
    }
}

fn move_pos(
    doc: &Doc,
    pos: &PosRef,
    dir: i64,
    line: Option<&super::line::LineRef>,
) -> Option<PosRef> {
    let p = pos.get();
    if dir < 0 && p.ch == 0 {
        if p.line > doc.first_line() {
            Some(PosRef::new(doc.clip_pos(Pos::line_end(p.line - 1))))
        } else {
            None
        }
    } else if dir > 0
        && p.ch == {
            let len = match line {
                Some(l) => l.borrow().text.len(),
                None => doc.get_line_ref(p.line).borrow().text.len(),
            };
            len as i64
        }
    {
        if p.line < doc.first_line() + doc.size() as i64 - 1 {
            Some(PosRef::lc(p.line + 1, 0))
        } else {
            None
        }
    } else {
        Some(PosRef::lc(p.line, p.ch + dir))
    }
}

/// `selectAll(cm)` as a document operation.
pub(crate) fn select_all(doc: &Doc) {
    let a = PosRef::new(doc.clip_pos(Pos::new(doc.first_line(), 0)));
    let h = PosRef::new(doc.clip_pos(Pos::line_end(doc.last_line())));
    set_simple_selection(
        doc,
        a,
        Some(h),
        Some(&SelOptions {
            scroll: Some(false),
            ..Default::default()
        }),
    );
}
