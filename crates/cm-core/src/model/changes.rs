//! `src/model/changes.js`: applying changes, propagating them to linked documents,
//! undo/redo, and history rebasing.

use std::rc::Rc;

use super::change_measurement::{change_end, compute_sel_after_change};
use super::doc::{BeforeChangeHandler, Doc};
use super::document_data::update_doc;
use super::history::{
    ChangeEvent, HistEvent, HistRef, add_change_to_history, history_change_from_change,
    merge_old_spans, push_selection_to_history,
};
use super::selection::{Range, Sel, SelRange, new_sel};
use super::selection_updates::{SelOptions, set_selection, set_selection_no_undo, skip_atomic};
use super::spans::{NewSpans, remove_read_only_ranges, stretch_spans_over_change};
use crate::pos::{Pos, PosRef, cmp};
use crate::text::Text;

/// An `updateDoc`-style change object.
#[derive(Clone, Debug)]
pub(crate) struct Change {
    pub from: Pos,
    pub to: Pos,
    /// `None` only for the `markText` history pseudo-change.
    pub text: Option<Vec<Text>>,
    pub origin: Option<String>,
    pub full: bool,
}

impl Change {
    pub fn text(&self) -> &Vec<Text> {
        self.text.as_ref().expect("change with text")
    }
}

/// A change as `change`/`changes` handlers receive it (`{from, to, text, removed,
/// origin}`), after it was applied: `to` is the end of the replaced range in the
/// coordinates from before the change.
#[derive(Clone, Debug, PartialEq)]
pub struct ChangeObj {
    pub from: Pos,
    pub to: Pos,
    pub text: Vec<String>,
    pub removed: Vec<String>,
    pub origin: Option<String>,
}

/// The object `beforeChange` handlers receive.
#[derive(Clone, Debug)]
pub struct BeforeChange {
    pub from: Pos,
    pub to: Pos,
    pub text: Vec<String>,
    pub origin: Option<String>,
    pub canceled: bool,
    can_update: bool,
    pending: Option<PendingUpdate>,
}

#[derive(Clone, Debug)]
struct PendingUpdate {
    from: Option<Pos>,
    to: Option<Pos>,
    text: Option<Vec<String>>,
    origin: Option<Option<String>>,
}

impl BeforeChange {
    pub fn cancel(&mut self) {
        self.canceled = true;
    }
    /// Whether `update` exists (it does not for changes replayed from history).
    pub fn can_update(&self) -> bool {
        self.can_update
    }
    /// `change.update(from, to, text, origin)`. Positions are clipped to the
    /// document. `origin: Some(None)` sets it to undefined.
    ///
    /// # Panics
    /// When `can_update()` is false — calling the missing method throws in the JS.
    pub fn update(
        &mut self,
        from: Option<Pos>,
        to: Option<Pos>,
        text: Option<Vec<String>>,
        origin: Option<Option<String>>,
    ) {
        assert!(
            self.can_update,
            "TypeError: change.update is not a function"
        );
        let p = self.pending.get_or_insert(PendingUpdate {
            from: None,
            to: None,
            text: None,
            origin: None,
        });
        if from.is_some() {
            p.from = from;
        }
        if to.is_some() {
            p.to = to;
        }
        if text.is_some() {
            p.text = text;
        }
        if origin.is_some() {
            p.origin = origin;
        }
    }
}

/// `filterChange(doc, change, update)`: run `beforeChange` handlers. `None` when a
/// handler cancelled the change. Note the result drops `full`, like the JS.
fn filter_change(doc: &Doc, change: &Change, update: bool) -> Option<Change> {
    let handlers: Vec<Rc<BeforeChangeHandler>> = doc
        .inner()
        .before_change
        .snapshot()
        .iter()
        .map(|e| e.f.clone())
        .collect();
    let mut obj = BeforeChange {
        from: change.from,
        to: change.to,
        text: change
            .text()
            .iter()
            .map(|t| crate::text::from_u16(t))
            .collect(),
        origin: change.origin.clone(),
        canceled: false,
        can_update: update,
        pending: None,
    };
    let mut text_u16: Vec<Text> = change.text().clone();
    // `signal(doc, "beforeChange", …)`, then `if (doc.cm) signal(doc.cm, …)`: the
    // editor's handlers are looked up after the document's ran.
    let mut editor_handlers = None;
    let mut i = 0;
    loop {
        let h = match handlers.get(i) {
            Some(h) => h.clone(),
            None => {
                let eh = editor_handlers.get_or_insert_with(|| {
                    doc.sink()
                        .map(|s| s.before_change_handlers())
                        .unwrap_or_default()
                });
                match eh.get(i - handlers.len()) {
                    Some(h) => h.clone(),
                    None => break,
                }
            }
        };
        i += 1;
        h(doc, &mut obj);
        // Apply updates the way the JS closure does: immediately, with clipping.
        if let Some(p) = obj.pending.take() {
            if let Some(f) = p.from {
                obj.from = doc.clip_pos(f);
            }
            if let Some(t) = p.to {
                obj.to = doc.clip_pos(t);
            }
            if let Some(t) = p.text {
                text_u16 = t.iter().map(|s| crate::text::to_u16(s)).collect();
                obj.text = t;
            }
            if let Some(o) = p.origin {
                obj.origin = o;
            }
        }
    }
    if obj.canceled {
        return None;
    }
    Some(Change {
        from: obj.from,
        to: obj.to,
        text: Some(text_u16),
        origin: obj.origin,
        full: false,
    })
}

/// `makeChange(doc, change, ignoreReadOnly)`.
pub(crate) fn make_change(doc: &Doc, change: Change, ignore_read_only: bool) {
    // `if (doc.cm.state.suppressEdits) return`
    if doc.suppress_edits() {
        return;
    }
    doc.in_op(|| {
        let mut change = change;
        if doc.has_before_change() {
            match filter_change(doc, &change, true) {
                Some(c) => change = c,
                None => return,
            }
        }
        // Possibly split or suppress the update based on the presence of read-only
        // spans in its range.
        let split = if ignore_read_only {
            None
        } else {
            remove_read_only_ranges(doc, change.from, change.to)
        };
        match split {
            Some(split) => {
                for i in (0..split.len()).rev() {
                    let text = if i > 0 {
                        vec![Vec::new()]
                    } else {
                        change.text().clone()
                    };
                    make_change_inner(
                        doc,
                        Change {
                            from: split[i].0,
                            to: split[i].1,
                            text: Some(text),
                            origin: change.origin.clone(),
                            full: false,
                        },
                    );
                }
            }
            None => make_change_inner(doc, change),
        }
    })
}

fn make_change_inner(doc: &Doc, change: Change) {
    let text = change.text();
    if text.len() == 1 && text[0].is_empty() && cmp(&change.from, &change.to).is_eq() {
        return;
    }
    let sel_after = compute_sel_after_change(doc, &change);
    add_change_to_history(doc, &change, sel_after.clone(), doc.op_id());

    let spans = stretch_spans_over_change(doc, &change);
    make_change_single_doc(doc, &change, Some(sel_after), spans);
    let mut rebased: Vec<HistRef> = Vec::new();
    for (d, shared_hist) in doc.linked_docs(false) {
        let h = d.history_rc();
        if !shared_hist && !rebased.iter().any(|r| Rc::ptr_eq(r, &h)) {
            rebase_hist(&h, &change);
            rebased.push(h);
        }
        let spans = stretch_spans_over_change(&d, &change);
        make_change_single_doc(&d, &change, None, spans);
    }
}

/// `makeChangeFromHistory(doc, type, allowSelectionOnly)`; `undo == false` is redo.
pub(crate) fn make_change_from_history(doc: &Doc, undo: bool, allow_selection_only: bool) {
    let suppress = doc.suppress_edits();
    if suppress && !allow_selection_only {
        return;
    }
    let hist_rc = doc.history_rc();
    let ty = if undo { "undo" } else { "redo" };
    let mut sel_after: Sel = doc.inner().sel.clone();

    // Verify that there is a useable event (so that ctrl-z won't needlessly clear
    // selection events).
    {
        let cur_sel = doc.inner().sel.clone();
        let hist = hist_rc.borrow();
        let source = if undo { &hist.done } else { &hist.undone };
        let usable = source.iter().any(|e| match e {
            HistEvent::Sel(s) => allow_selection_only && !super::selection::sel_equals(s, &cur_sel),
            HistEvent::Change(_) => !allow_selection_only,
        });
        if !usable {
            return;
        }
    }
    {
        let mut hist = hist_rc.borrow_mut();
        hist.last_origin = None;
        hist.last_sel_origin = None;
    }

    let event: ChangeEvent;
    loop {
        let popped = {
            let mut hist = hist_rc.borrow_mut();
            let source = if undo {
                &mut hist.done
            } else {
                &mut hist.undone
            };
            source.pop().expect("usable event exists")
        };
        match popped {
            HistEvent::Sel(s) => {
                {
                    let mut hist = hist_rc.borrow_mut();
                    if undo {
                        push_selection_to_history(&s, &mut hist.undone)
                    } else {
                        hist.push_sel_done(&s)
                    }
                }
                let cur_sel = doc.inner().sel.clone();
                if allow_selection_only && !super::selection::sel_equals(&s, &cur_sel) {
                    set_selection(
                        doc,
                        s,
                        Some(&SelOptions {
                            clear_redo: Some(false),
                            ..Default::default()
                        }),
                    );
                    return;
                }
                sel_after = s;
            }
            HistEvent::Change(c) => {
                if suppress {
                    // `else if (suppress) { source.push(event); return }`
                    let mut hist = hist_rc.borrow_mut();
                    let source = if undo {
                        &mut hist.done
                    } else {
                        &mut hist.undone
                    };
                    source.push(HistEvent::Change(c));
                    return;
                }
                event = c;
                break;
            }
        }
    }

    // Build up a reverse change object to add to the opposite history stack.
    let dest_idx = {
        let mut hist = hist_rc.borrow_mut();
        let generation = hist.generation;
        if undo {
            push_selection_to_history(&sel_after, &mut hist.undone)
        } else {
            hist.push_sel_done(&sel_after)
        }
        let dest = if undo {
            &mut hist.undone
        } else {
            &mut hist.done
        };
        dest.push(HistEvent::Change(ChangeEvent {
            changes: Vec::new(),
            generation: Some(generation),
        }));
        let idx = dest.len() - 1;
        hist.generation = match event.generation {
            Some(g) if g != 0 => g,
            _ => {
                hist.max_generation += 1;
                hist.max_generation
            }
        };
        idx
    };

    let filter = doc.has_before_change();

    for i in (0..event.changes.len()).rev() {
        let hchange = &event.changes[i];
        let change = Change {
            from: hchange.from,
            to: hchange.to,
            text: Some(hchange.text.clone()),
            origin: Some(ty.to_string()),
            full: false,
        };
        if filter && filter_change(doc, &change, false).is_none() {
            let mut hist = hist_rc.borrow_mut();
            let source = if undo {
                &mut hist.done
            } else {
                &mut hist.undone
            };
            source.clear();
            return;
        }

        let anti = history_change_from_change(doc, &change);
        {
            let mut hist = hist_rc.borrow_mut();
            let dest = if undo {
                &mut hist.undone
            } else {
                &mut hist.done
            };
            if let Some(HistEvent::Change(ev)) = dest.get_mut(dest_idx) {
                ev.changes.push(anti);
            }
        }

        let after: Option<Sel> = if i > 0 {
            Some(compute_sel_after_change(doc, &change))
        } else {
            let hist = hist_rc.borrow();
            let last = if undo {
                hist.lst_done()
            } else {
                hist.undone.last()
            };
            match last {
                Some(HistEvent::Sel(s)) => Some(s.clone()),
                None => None,
                Some(HistEvent::Change(_)) => panic!("JS TypeError: sel.primary is not a function"),
            }
        };
        let spans = merge_old_spans(doc, hchange, &change);
        make_change_single_doc(doc, &change, after, spans);
        if i == 0 && doc.has_editor() {
            // `if (!i && doc.cm) doc.cm.scrollIntoView({from, to: changeEnd(change)})`,
            // applied by the view when the operation ends.
            doc.request_scroll_into_view(change.from, change_end(&change));
        }

        // Propagate to the linked documents.
        let mut rebased: Vec<HistRef> = Vec::new();
        for (d, shared_hist) in doc.linked_docs(false) {
            let h = d.history_rc();
            if !shared_hist && !rebased.iter().any(|r| Rc::ptr_eq(r, &h)) {
                rebase_hist(&h, &change);
                rebased.push(h);
            }
            let spans = merge_old_spans(&d, hchange, &change);
            make_change_single_doc(&d, &change, None, spans);
        }
    }
}

/// `shiftDoc(doc, distance)`: sub-views move when text is added above them.
fn shift_doc(doc: &Doc, distance: i64) {
    if distance == 0 {
        return;
    }
    {
        let mut inner = doc.inner_mut();
        inner.first += distance;
        let sel = inner.sel.clone();
        let ranges = sel
            .ranges
            .iter()
            .map(|r| {
                Range::new(
                    PosRef::lc(r.anchor.line() + distance, r.anchor.ch()),
                    PosRef::lc(r.head.line() + distance, r.head.ch()),
                )
            })
            .collect();
        inner.sel = new_sel(ranges, sel.prim_index);
    }
    // `if (doc.cm) { regChange(…); regLineChange(…, "gutter") … }`
    if let Some(s) = doc.sink() {
        s.doc_shifted(doc, distance);
    }
}

/// `makeChangeSingleDoc(doc, change, selAfter, spans)`: apply to one document,
/// clipping the change to a sub-view's range.
fn make_change_single_doc(
    doc: &Doc,
    change: &Change,
    sel_after: Option<Sel>,
    spans: Option<NewSpans>,
) {
    // `if (doc.cm && !doc.cm.curOp) return operation(doc.cm, makeChangeSingleDoc)(…)`
    doc.in_op(|| make_change_single_doc_in_op(doc, change, sel_after, spans))
}

fn make_change_single_doc_in_op(
    doc: &Doc,
    change: &Change,
    sel_after: Option<Sel>,
    spans: Option<NewSpans>,
) {
    let text = change.text();
    if change.to.line < doc.first_line() {
        shift_doc(
            doc,
            text.len() as i64 - 1 - (change.to.line - change.from.line),
        );
        return;
    }
    if change.from.line > doc.last_line() {
        return;
    }
    let mut change = change.clone();
    // Clip the change to the size of this doc.
    if change.from.line < doc.first_line() {
        let shift = text.len() as i64 - 1 - (doc.first_line() - change.from.line);
        shift_doc(doc, shift);
        change = Change {
            from: Pos::new(doc.first_line(), 0),
            to: Pos::new(change.to.line + shift, change.to.ch),
            text: Some(vec![text.last().cloned().unwrap_or_default()]),
            origin: change.origin.clone(),
            full: false,
        };
    }
    let last = doc.last_line();
    if change.to.line > last {
        let first_text = change.text()[0].clone();
        change = Change {
            from: change.from,
            to: Pos::new(last, doc.get_line_ref(last).borrow().text.len() as i64),
            text: Some(vec![first_text]),
            origin: change.origin.clone(),
            full: false,
        };
    }
    let obj = Rc::new(ChangeObj {
        from: change.from,
        to: change.to,
        text: change
            .text()
            .iter()
            .map(|t| crate::text::from_u16(t))
            .collect(),
        removed: doc
            .get_between(change.from, change.to)
            .iter()
            .map(|t| crate::text::from_u16(t))
            .collect(),
        origin: change.origin.clone(),
    });
    let sel_after = match sel_after {
        Some(s) => s,
        None => compute_sel_after_change(doc, &change),
    };
    let sink = doc.sink();
    // makeChangeSingleDocInEditor: `if (doc.sel.contains(change.from, change.to) > -1)
    // signalCursorActivity(cm)`, with the selection from before the change.
    if let Some(s) = &sink
        && doc.inner().sel.contains(&change.from, Some(&change.to)) > -1
    {
        s.signal_cursor_activity();
    }
    update_doc(doc, &change, spans, Some(&obj));
    if let Some(s) = &sink {
        s.change_applied(doc, &obj, change.full);
    }
    set_selection_no_undo(
        doc,
        sel_after,
        Some(&SelOptions {
            scroll: Some(false),
            ..Default::default()
        }),
    );

    if doc.inner().cant_edit {
        // `skipAtomic` always returns a truthy position, so this always resets.
        skip_atomic(doc, &PosRef::lc(doc.first_line(), 0), None, None, false);
        doc.inner_mut().cant_edit = false;
    }
}

/// `replaceRange(doc, code, from, to, origin)` with clipped positions.
pub(crate) fn replace_range(
    doc: &Doc,
    code: Vec<Text>,
    from: Pos,
    to: Pos,
    origin: Option<String>,
) {
    let (from, to) = if cmp(&to, &from).is_lt() {
        (to, from)
    } else {
        (from, to)
    };
    make_change(
        doc,
        Change {
            from,
            to,
            text: Some(code),
            origin,
            full: false,
        },
        false,
    );
}

fn rebase_hist_sel_single(pos: &PosRef, from: i64, to: i64, diff: i64) {
    if to < pos.line() {
        pos.set_line(pos.line() + diff);
    } else if from < pos.line() {
        pos.set_line(from);
        pos.set_ch(0);
    }
}

/// `rebaseHistArray(array, from, to, diff)`. Selection events are copied on first
/// touch (`copied`), then mutated in place — which also mutates any live selection
/// that shares the copied object, exactly like the JS.
fn rebase_hist_array(array: &mut Vec<HistEvent>, from: i64, to: i64, diff: i64) {
    let mut i = 0;
    while i < array.len() {
        let mut ok = true;
        match &mut array[i] {
            HistEvent::Sel(sub) => {
                if !sub.copied.get() {
                    let copy = Rc::new(sub.deep_copy());
                    copy.copied.set(true);
                    *sub = copy;
                }
                for r in &sub.ranges {
                    rebase_hist_sel_single(&r.anchor, from, to, diff);
                    rebase_hist_sel_single(&r.head, from, to, diff);
                }
                i += 1;
                continue;
            }
            HistEvent::Change(ev) => {
                for cur in ev.changes.iter_mut() {
                    if to < cur.from.line {
                        cur.from = Pos::new(cur.from.line + diff, cur.from.ch);
                        cur.to = Pos::new(cur.to.line + diff, cur.to.ch);
                    } else if from <= cur.to.line {
                        ok = false;
                        break;
                    }
                }
            }
        }
        if !ok {
            array.drain(0..=i);
            // `i = 0` followed by the loop's `++i`: index 0 is skipped.
            i = 1;
            continue;
        }
        i += 1;
    }
}

/// `rebaseHist(hist, change)`.
fn rebase_hist(hist: &HistRef, change: &Change) {
    let from = change.from.line;
    let to = change.to.line;
    let diff = change.text().len() as i64 - (to - from) - 1;
    let mut h = hist.borrow_mut();
    rebase_hist_array(&mut h.done, from, to, diff);
    rebase_hist_array(&mut h.undone, from, to, diff);
}

/// The object `beforeSelectionChange` handlers receive.
#[derive(Clone, Debug)]
pub struct BeforeSelectionChange {
    pub ranges: Vec<SelRange>,
    pub origin: Option<String>,
    pub(crate) updated: Option<Vec<SelRange>>,
}

impl BeforeSelectionChange {
    pub(crate) fn new(ranges: Vec<SelRange>, origin: Option<String>) -> Self {
        BeforeSelectionChange {
            ranges,
            origin,
            updated: None,
        }
    }
    /// `obj.update(ranges)`: replace the new selection. Positions are clipped when
    /// the update is applied.
    pub fn update(&mut self, ranges: Vec<SelRange>) {
        self.updated = Some(ranges);
    }
}
