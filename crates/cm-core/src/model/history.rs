//! `src/model/history.js`: undo/redo stacks, selection history and event merging.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use super::change_measurement::change_end;
use super::changes::Change;
use super::doc::Doc;
use super::selection::{Range, Sel, new_sel};
use super::spans::{NewSpans, SpanArr, stretch_spans_over_change};
use crate::clock;
use crate::pos::{Pos, PosRef, cmp};
use crate::text::Text;

/// One change inside a history event (`{from, to, text, spans_<docid>}`).
#[derive(Clone)]
pub(crate) struct HistChange {
    pub from: Pos,
    pub to: Pos,
    pub text: Vec<Text>,
    /// `change["spans_" + doc.id][n]`, per document id.
    pub spans: BTreeMap<u32, BTreeMap<usize, SpanArr>>,
}

#[derive(Clone)]
pub(crate) struct ChangeEvent {
    pub changes: Vec<HistChange>,
    /// `undefined` for events rebuilt by `copyHistoryArray`.
    pub generation: Option<u64>,
}

#[derive(Clone)]
pub(crate) enum HistEvent {
    Sel(Sel),
    Change(ChangeEvent),
}

impl HistEvent {
    pub fn is_sel(&self) -> bool {
        matches!(self, HistEvent::Sel(_))
    }
}

pub(crate) struct History {
    pub done: Vec<HistEvent>,
    /// The JS `done` array's `"-1"` property. `hist.done[hist.done.length - 1] = sel`
    /// on an *empty* array creates it, and `lst(hist.done)` (`arr[arr.length - 1]`)
    /// returns it whenever the array is empty again. Lives as long as the array.
    pub done_ghost: Option<HistEvent>,
    pub undone: Vec<HistEvent>,
    /// `Infinity` by default.
    pub undo_depth: f64,
    pub last_mod_time: f64,
    pub last_sel_time: f64,
    /// Operation ids; `None` stands for both `null` and `NaN` — neither ever compares
    /// equal to a real operation id.
    pub last_op: Option<u64>,
    pub last_sel_op: Option<u64>,
    pub last_origin: Option<String>,
    pub last_sel_origin: Option<String>,
    pub generation: u64,
    pub max_generation: u64,
}

pub(crate) type HistRef = Rc<RefCell<History>>;

impl History {
    /// `new History(prev)`.
    pub fn new(prev: Option<&History>) -> History {
        let generation = prev.map(|p| p.max_generation).unwrap_or(1);
        History {
            done: Vec::new(),
            done_ghost: None,
            undone: Vec::new(),
            undo_depth: prev.map(|p| p.undo_depth).unwrap_or(f64::INFINITY),
            last_mod_time: 0.0,
            last_sel_time: 0.0,
            last_op: None,
            last_sel_op: None,
            last_origin: None,
            last_sel_origin: None,
            generation,
            max_generation: generation,
        }
    }
}

impl History {
    /// `lst(hist.done)`.
    pub fn lst_done(&self) -> Option<&HistEvent> {
        self.done.last().or(self.done_ghost.as_ref())
    }
    /// `hist.done[hist.done.length - 1] = ev`.
    pub fn set_last_done(&mut self, ev: HistEvent) {
        match self.done.last_mut() {
            Some(l) => *l = ev,
            None => self.done_ghost = Some(ev),
        }
    }
    /// `pushSelectionToHistory(sel, hist.done)`.
    pub fn push_sel_done(&mut self, sel: &Sel) {
        if let Some(HistEvent::Sel(top)) = self.lst_done() {
            if super::selection::sel_equals(top, sel) {
                return;
            }
        }
        self.done.push(HistEvent::Sel(sel.clone()));
    }
}

pub(crate) fn op_eq(a: Option<u64>, b: Option<u64>) -> bool {
    a.is_some() && a == b
}

fn truthy(s: &Option<String>) -> bool {
    s.as_deref().is_some_and(|s| !s.is_empty())
}

fn first_char(s: &Option<String>) -> Option<char> {
    s.as_deref().and_then(|s| s.chars().next())
}

/// `historyChangeFromChange(doc, change)`.
pub(crate) fn history_change_from_change(doc: &Doc, change: &Change) -> HistChange {
    let mut hist = HistChange {
        from: Pos::new(change.from.line, change.from.ch),
        to: change_end(change),
        text: doc.get_between(change.from, change.to),
        spans: BTreeMap::new(),
    };
    attach_local_spans(doc, &mut hist, change.from.line, change.to.line + 1);
    for (d, _) in doc.linked_docs(true) {
        attach_local_spans(&d, &mut hist, change.from.line, change.to.line + 1);
    }
    hist
}

/// Pop all selection events off the end of a history array.
fn clear_selection_events(array: &mut Vec<HistEvent>) {
    while array.last().is_some_and(|e| e.is_sel()) {
        array.pop();
    }
}

/// `lastChangeEvent(hist, force)`: index of the top change event.
fn last_change_event(hist: &mut History, force: bool) -> Option<usize> {
    if force {
        clear_selection_events(&mut hist.done);
        return match hist.lst_done() {
            Some(HistEvent::Change(_)) => Some(hist.done.len() - 1),
            // The `"-1"` ghost is a selection: truthy, and `lst(cur.changes)` throws.
            Some(HistEvent::Sel(_)) => {
                panic!("JS TypeError: Cannot read properties of undefined (reading 'length')")
            }
            None => None,
        };
    }
    let n = hist.done.len();
    if n > 0 && !hist.done[n - 1].is_sel() {
        Some(n - 1)
    } else if n > 1 && !hist.done[n - 2].is_sel() {
        hist.done.pop();
        Some(n - 2)
    } else {
        None
    }
}

/// `addChangeToHistory(doc, change, selAfter, opId)`.
pub(crate) fn add_change_to_history(
    doc: &Doc,
    change: &Change,
    sel_after: Sel,
    op_id: Option<u64>,
) {
    let hist_rc = doc.history_rc();
    let delay = doc.history_event_delay();
    let time = clock::now();
    let (merge_into, force) = {
        let mut hist = hist_rc.borrow_mut();
        hist.undone.clear();
        let force = op_eq(hist.last_op, op_id);
        let mergeable = force
            || hist.last_origin == change.origin
                && truthy(&change.origin)
                && ((first_char(&change.origin) == Some('+') && hist.last_mod_time > time - delay)
                    || first_char(&change.origin) == Some('*'));
        let cur = if mergeable {
            last_change_event(&mut hist, force)
        } else {
            None
        };
        (cur, force)
    };
    let _ = force;
    let mut merged = false;
    if let Some(idx) = merge_into {
        // Merge this change into the last event.
        let simple = {
            let hist = hist_rc.borrow();
            let HistEvent::Change(cur) = &hist.done[idx] else {
                unreachable!()
            };
            let last = cur.changes.last().expect("event has changes");
            cmp(&change.from, &change.to).is_eq() && cmp(&change.from, &last.to).is_eq()
        };
        merged = true;
        if simple {
            // Optimized case for simple insertion: don't add a sub-event per char.
            let end = change_end(change);
            let mut hist = hist_rc.borrow_mut();
            let HistEvent::Change(cur) = &mut hist.done[idx] else {
                unreachable!()
            };
            cur.changes.last_mut().unwrap().to = end;
        } else {
            let hc = history_change_from_change(doc, change);
            let mut hist = hist_rc.borrow_mut();
            let HistEvent::Change(cur) = &mut hist.done[idx] else {
                unreachable!()
            };
            cur.changes.push(hc);
        }
    } else {
        // Can not be merged, start a new event.
        let need_sel = {
            let hist = hist_rc.borrow();
            !hist.lst_done().is_some_and(|e| e.is_sel())
        };
        if need_sel {
            let sel = doc.inner().sel.clone();
            hist_rc.borrow_mut().push_sel_done(&sel);
        }
        let hc = history_change_from_change(doc, change);
        let mut hist = hist_rc.borrow_mut();
        let generation = hist.generation;
        hist.done.push(HistEvent::Change(ChangeEvent {
            changes: vec![hc],
            generation: Some(generation),
        }));
        while hist.done.len() as f64 > hist.undo_depth {
            hist.done.remove(0);
            // `hist.done[0].ranges` throws a TypeError in the JS when the array is now
            // empty (only possible with undoDepth 0).
            if !hist
                .done
                .first()
                .expect("JS TypeError: done[0] is undefined")
                .is_sel()
            {
                hist.done.remove(0);
            }
        }
    }
    let mut hist = hist_rc.borrow_mut();
    hist.done.push(HistEvent::Sel(sel_after));
    hist.max_generation += 1;
    hist.generation = hist.max_generation;
    hist.last_mod_time = time;
    hist.last_sel_time = time;
    hist.last_op = op_id;
    hist.last_sel_op = op_id;
    hist.last_origin = change.origin.clone();
    hist.last_sel_origin = change.origin.clone();
    let _ = merged; // `signal(doc, "historyAdded")` is not ported.
}

fn selection_event_can_be_merged(
    delay: f64,
    hist: &History,
    origin: &str,
    prev: Option<&HistEvent>,
    sel: &Sel,
) -> bool {
    let ch = origin.chars().next();
    if ch == Some('*') {
        return true;
    }
    if ch != Some('+') {
        return false;
    }
    let Some(HistEvent::Sel(prev)) = prev else {
        panic!("JS TypeError: Cannot read properties of undefined (reading 'length')");
    };
    prev.ranges.len() == sel.ranges.len()
        && prev.something_selected() == sel.something_selected()
        && clock::now() - hist.last_sel_time <= delay
}

/// Options a selection update can carry (`{origin, bias, scroll, clearRedo}`).
pub(crate) struct SelHistOpts<'a> {
    pub origin: Option<&'a str>,
    pub clear_redo: Option<bool>,
}

/// `addSelectionToHistory(doc, sel, opId, options)`; `options == None` is the JS
/// `undefined` (which, notably, does *not* clear redo selection events).
pub(crate) fn add_selection_to_history(
    doc: &Doc,
    sel: Sel,
    op_id: Option<u64>,
    options: Option<SelHistOpts>,
) {
    let hist_rc = doc.history_rc();
    let delay = doc.history_event_delay();
    let mut hist = hist_rc.borrow_mut();
    let origin: Option<String> = options.as_ref().and_then(|o| o.origin.map(str::to_string));
    let origin_truthy = truthy(&origin);
    let replace = op_eq(op_id, hist.last_sel_op)
        || origin_truthy
            && hist.last_sel_origin == origin
            && (hist.last_mod_time == hist.last_sel_time && hist.last_origin == origin
                || selection_event_can_be_merged(
                    delay,
                    &hist,
                    origin.as_deref().unwrap(),
                    hist.lst_done(),
                    &sel,
                ));
    if replace {
        hist.set_last_done(HistEvent::Sel(sel));
    } else {
        hist.push_sel_done(&sel);
    }
    hist.last_sel_time = clock::now();
    hist.last_sel_origin = origin;
    hist.last_sel_op = op_id;
    if options
        .as_ref()
        .is_some_and(|o| o.clear_redo != Some(false))
    {
        clear_selection_events(&mut hist.undone);
    }
}

/// `pushSelectionToHistory(sel, dest)`.
pub(crate) fn push_selection_to_history(sel: &Sel, dest: &mut Vec<HistEvent>) {
    if let Some(HistEvent::Sel(top)) = dest.last() {
        if super::selection::sel_equals(top, sel) {
            return;
        }
    }
    dest.push(HistEvent::Sel(sel.clone()));
}

/// `attachLocalSpans(doc, change, from, to)`.
fn attach_local_spans(doc: &Doc, change: &mut HistChange, from: i64, to: i64) {
    let start = doc.first_line().max(from);
    let end = (doc.first_line() + doc.size() as i64).min(to);
    let id = doc.id();
    let mut n = 0usize;
    for line in doc.iter_lines(start, end) {
        if let Some(spans) = &line.borrow().marked_spans {
            change.spans.entry(id).or_default().insert(n, spans.clone());
        }
        n += 1;
    }
}

/// `removeClearedSpans(spans)`: the same array object when nothing was cleared.
fn remove_cleared_spans(spans: Option<&SpanArr>) -> Option<SpanArr> {
    let arr = spans?;
    let spans = arr.get();
    let out: Vec<_> = spans
        .iter()
        .filter(|s| !s.marker.data().explicitly_cleared)
        .cloned()
        .collect();
    if out.len() == spans.len() {
        Some(arr.clone())
    } else if out.is_empty() {
        None
    } else {
        Some(SpanArr::new(out))
    }
}

/// `getOldSpans(doc, change)`.
fn get_old_spans(doc: &Doc, change: &HistChange) -> Option<NewSpans> {
    let found = change.spans.get(&doc.id())?;
    Some(
        (0..change.text.len())
            .map(|i| remove_cleared_spans(found.get(&i)))
            .collect(),
    )
}

/// `mergeOldSpans(doc, change)`: combine the spans recorded in history with the
/// ones computed for the current document, so that undoing a deletion brings back
/// the markers that were inside it.
pub(crate) fn merge_old_spans(
    doc: &Doc,
    hchange: &HistChange,
    change: &Change,
) -> Option<NewSpans> {
    let old = get_old_spans(doc, hchange);
    let stretched = stretch_spans_over_change(doc, change);
    let Some(mut old) = old else { return stretched };
    let Some(stretched) = stretched else {
        return Some(old);
    };
    for i in 0..old.len() {
        let stretch_cur = stretched.get(i).cloned().flatten();
        match (&mut old[i], stretch_cur) {
            (Some(old_cur), Some(stretch_cur)) => {
                // Pushes into the recorded array object itself (shared by reference).
                for span in stretch_cur.get() {
                    if old_cur.get().iter().any(|o| o.marker.ptr_eq(&span.marker)) {
                        continue;
                    }
                    old_cur.push(span);
                }
            }
            (slot @ None, Some(stretch_cur)) => *slot = Some(stretch_cur),
            _ => {}
        }
    }
    Some(old)
}

/// `copyHistoryArray(events, newGroup, instantiateSel)`. With `new_group`, the span
/// records of those document ids are *moved* from `events` into the copy.
pub(crate) fn copy_history_array(
    events: &mut [HistEvent],
    new_group: Option<&[u32]>,
    instantiate_sel: bool,
) -> Vec<HistEvent> {
    let mut copy = Vec::new();
    for event in events.iter_mut() {
        match event {
            HistEvent::Sel(sel) => {
                copy.push(HistEvent::Sel(if instantiate_sel {
                    Rc::new(sel.deep_copy())
                } else {
                    sel.clone()
                }));
            }
            HistEvent::Change(ev) => {
                let mut new_changes = Vec::new();
                for change in ev.changes.iter_mut() {
                    let mut nc = HistChange {
                        from: change.from,
                        to: change.to,
                        text: change.text.clone(),
                        spans: BTreeMap::new(),
                    };
                    if let Some(group) = new_group {
                        let ids: Vec<u32> = change
                            .spans
                            .keys()
                            .copied()
                            .filter(|id| group.contains(id))
                            .collect();
                        for id in ids {
                            if let Some(s) = change.spans.remove(&id) {
                                nc.spans.insert(id, s);
                            }
                        }
                    }
                    new_changes.push(nc);
                }
                copy.push(HistEvent::Change(ChangeEvent {
                    changes: new_changes,
                    generation: None,
                }));
            }
        }
    }
    copy
}

/// `historySize()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistorySize {
    pub undo: usize,
    pub redo: usize,
}

/// One change in a [`HistoryData`] event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryChangeData {
    pub from: Pos,
    pub to: Pos,
    /// The replaced text, one entry per line, as UTF-16 (lossless).
    pub text: Vec<Text>,
}

impl HistoryChangeData {
    pub fn text_lines(&self) -> Vec<String> {
        self.text.iter().map(|t| crate::text::from_u16(t)).collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryEventData {
    Selection(super::selection::Selection),
    Changes(Vec<HistoryChangeData>),
}

/// The JSON-safe history returned by `getHistory` and accepted by `setHistory`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryData {
    pub done: Vec<HistoryEventData>,
    pub undone: Vec<HistoryEventData>,
}

pub(crate) fn events_to_data(events: &[HistEvent]) -> Vec<HistoryEventData> {
    events
        .iter()
        .map(|e| match e {
            HistEvent::Sel(s) => HistoryEventData::Selection(s.snapshot()),
            HistEvent::Change(c) => HistoryEventData::Changes(
                c.changes
                    .iter()
                    .map(|ch| HistoryChangeData {
                        from: ch.from,
                        to: ch.to,
                        text: ch.text.clone(),
                    })
                    .collect(),
            ),
        })
        .collect()
}

/// `setHistory` side: `copyHistoryArray(data, null, true)` — selections are
/// instantiated with fresh positions (dropping `sticky`, as `copyPos` does).
pub(crate) fn data_to_events(data: &[HistoryEventData]) -> Vec<HistEvent> {
    data.iter()
        .map(|e| match e {
            HistoryEventData::Selection(s) => HistEvent::Sel(new_sel(
                s.ranges
                    .iter()
                    .map(|r| {
                        Range::new(
                            PosRef::lc(r.anchor.line, r.anchor.ch),
                            PosRef::lc(r.head.line, r.head.ch),
                        )
                    })
                    .collect(),
                s.primary,
            )),
            HistoryEventData::Changes(cs) => HistEvent::Change(ChangeEvent {
                changes: cs
                    .iter()
                    .map(|c| HistChange {
                        from: c.from,
                        to: c.to,
                        text: c.text.clone(),
                        spans: BTreeMap::new(),
                    })
                    .collect(),
                generation: None,
            }),
        })
        .collect()
}
