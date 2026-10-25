//! `src/model/change_measurement.js`.

use std::cmp::Ordering;

use super::changes::Change;
use super::doc::Doc;
use super::selection::{Range, Sel, new_sel, normalize_selection};
use crate::pos::{Pos, PosRef, cmp};

/// `changeEnd(change)`: the post-change end position.
pub(crate) fn change_end(change: &Change) -> Pos {
    let Some(text) = &change.text else {
        return change.to;
    };
    // `lst(change.text).length` on an empty array is a TypeError in the JS; an empty
    // text array only comes out of history built from positions outside the document.
    let last_len = text
        .last()
        .map(|t| t.len() as i64)
        .expect("JS TypeError: Cannot read properties of undefined (reading 'length')");
    Pos::new(
        change.from.line + text.len() as i64 - 1,
        last_len + if text.len() == 1 { change.from.ch } else { 0 },
    )
}

/// `adjustForChange(pos, change)`. Returns the *same* object when the position is
/// before the change, a fresh one otherwise.
fn adjust_for_change(pos: &PosRef, change: &Change) -> PosRef {
    let p = pos.get();
    if cmp(&p, &change.from) == Ordering::Less {
        return pos.clone();
    }
    if cmp(&p, &change.to) != Ordering::Greater {
        return PosRef::new(change_end(change));
    }
    let text_len = change.text.as_ref().map(|t| t.len() as i64).unwrap_or(0);
    let line = p.line + text_len - (change.to.line - change.from.line) - 1;
    let mut ch = p.ch;
    if p.line == change.to.line {
        ch += change_end(change).ch - change.to.ch;
    }
    PosRef::lc(line, ch)
}

/// `computeSelAfterChange(doc, change)`.
pub(crate) fn compute_sel_after_change(doc: &Doc, change: &Change) -> Sel {
    let sel = doc.inner().sel.clone();
    let out: Vec<Range> = sel
        .ranges
        .iter()
        .map(|r| {
            Range::new(
                adjust_for_change(&r.anchor, change),
                adjust_for_change(&r.head, change),
            )
        })
        .collect();
    normalize_selection(doc.selections_may_touch(), out, sel.prim_index)
}

fn offset_pos(pos: &Pos, old: &Pos, nw: &Pos) -> Pos {
    if pos.line == old.line {
        Pos::new(nw.line, pos.ch - old.ch + nw.ch)
    } else {
        Pos::new(nw.line + (pos.line - old.line), pos.ch)
    }
}

/// `computeReplacedSel(doc, changes, hint)`: hint `"around"` selects the inserted
/// text, anything else collapses to its start. The result is *not* normalized.
pub(crate) fn compute_replaced_sel(doc: &Doc, changes: &[Change], hint: &str) -> Sel {
    let sel = doc.inner().sel.clone();
    let first = doc.first_line();
    let mut out = Vec::new();
    let mut old_prev = Pos::new(first, 0);
    let mut new_prev = old_prev;
    for (i, change) in changes.iter().enumerate() {
        let from = offset_pos(&change.from, &old_prev, &new_prev);
        let to = offset_pos(&change_end(change), &old_prev, &new_prev);
        old_prev = change.to;
        new_prev = to;
        if hint == "around" {
            let range = &sel.ranges[i];
            let inv = cmp(&range.head.get(), &range.anchor.get()) == Ordering::Less;
            out.push(Range::new(
                PosRef::new(if inv { to } else { from }),
                PosRef::new(if inv { from } else { to }),
            ));
        } else {
            let p = PosRef::new(from);
            out.push(Range::new(p.clone(), p));
        }
    }
    new_sel(out, sel.prim_index)
}
