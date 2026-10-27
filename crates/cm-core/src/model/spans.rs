//! `src/line/spans.js`: marked spans on lines, adjusting them over changes, read-only
//! range splitting and the collapsed-span ("visual line") helpers.
//!
//! The JS `sawReadOnlySpans` / `sawCollapsedSpans` globals are pure optimizations
//! (they skip scans that would find nothing) and are not ported.

use std::rc::Rc;

use super::changes::Change;
use super::doc::Doc;
use super::line::LineRef;
use super::mark_text::{FindL, MarkerType, TextMarker};
use crate::pos::{Pos, cmp, cmp_num};

/// `MarkedSpan(marker, from, to)`; `None` stands for the JS `null` ("continues past
/// this line").
#[derive(Clone)]
pub(crate) struct MarkedSpan {
    pub marker: TextMarker,
    pub from: Option<i64>,
    pub to: Option<i64>,
}

pub(crate) type Spans = Vec<MarkedSpan>;

/// A `markedSpans` *array object*. The JS shares these arrays by reference: history
/// events keep references to the arrays lines had, "gap" lines created by one change
/// share a single array, and `mergeOldSpans` pushes into recorded arrays in place.
/// Those aliasing effects are observable, so the array is a shared mutable object.
#[derive(Clone)]
pub(crate) struct SpanArr(Rc<std::cell::RefCell<Spans>>);

impl SpanArr {
    pub fn new(v: Spans) -> SpanArr {
        SpanArr(Rc::new(std::cell::RefCell::new(v)))
    }
    /// A snapshot of the current contents.
    pub fn get(&self) -> Spans {
        self.0.borrow().clone()
    }
    // Ported for completeness; only the (unported) view layer calls it.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }
    pub fn at(&self, i: usize) -> Option<MarkedSpan> {
        self.0.borrow().get(i).cloned()
    }
    pub fn push(&self, s: MarkedSpan) {
        self.0.borrow_mut().push(s)
    }
    pub fn ptr_eq(&self, o: &SpanArr) -> bool {
        Rc::ptr_eq(&self.0, &o.0)
    }
}

/// `getMarkedSpanFor(spans, marker)`.
pub(crate) fn get_marked_span_for(
    spans: Option<&SpanArr>,
    marker: &TextMarker,
) -> Option<MarkedSpan> {
    spans?
        .0
        .borrow()
        .iter()
        .find(|s| s.marker.ptr_eq(marker))
        .cloned()
}

fn find_in(spans: Option<&Spans>, marker: &TextMarker) -> Option<MarkedSpan> {
    spans?.iter().find(|s| s.marker.ptr_eq(marker)).cloned()
}

/// `removeMarkedSpan(spans, span)` where `span` is the first span of `marker`
/// (that is how every caller finds it). `None` when nothing is left.
pub(crate) fn remove_marked_span(spans: &SpanArr, marker: &TextMarker) -> Option<SpanArr> {
    let spans = spans.get();
    let idx = spans.iter().position(|s| s.marker.ptr_eq(marker));
    let r: Spans = spans
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != idx)
        .map(|(_, s)| s.clone())
        .collect();
    if r.is_empty() {
        None
    } else {
        Some(SpanArr::new(r))
    }
}

/// `addMarkedSpan(line, span, op)`. Inside an editor operation, an array this
/// operation already created is pushed to in place; otherwise the line gets a new
/// array (`concat`).
pub(crate) fn add_marked_span(doc: &Doc, line: &LineRef, span: MarkedSpan) {
    let marker = span.marker.clone();
    let in_op = doc.op_id().is_some();
    let existing = line.borrow().marked_spans.clone();
    let reuse = in_op
        && existing
            .as_ref()
            .is_some_and(|a| doc.inner().op_span_arrays.iter().any(|x| x.ptr_eq(a)));
    if reuse {
        existing.unwrap().push(span);
    } else {
        let mut v = existing.map(|a| a.get()).unwrap_or_default();
        v.push(span);
        let arr = SpanArr::new(v);
        if in_op {
            doc.inner_mut().op_span_arrays.push(arr.clone());
        }
        line.borrow_mut().marked_spans = Some(arr);
    }
    marker.attach_line(line);
}

fn marked_spans_before(old: Option<&Spans>, start_ch: i64, is_insert: bool) -> Option<Spans> {
    let mut nw: Option<Spans> = None;
    for span in old.into_iter().flatten() {
        let m = span.marker.data();
        let starts_before = match span.from {
            None => true,
            Some(f) => {
                if m.opts.inclusive_left {
                    f <= start_ch
                } else {
                    f < start_ch
                }
            }
        };
        if starts_before
            || span.from == Some(start_ch)
                && m.kind == MarkerType::Bookmark
                && (!is_insert || !m.opts.insert_left)
        {
            let ends_after = match span.to {
                None => true,
                Some(t) => {
                    if m.opts.inclusive_right {
                        t >= start_ch
                    } else {
                        t > start_ch
                    }
                }
            };
            nw.get_or_insert_with(Vec::new).push(MarkedSpan {
                marker: span.marker.clone(),
                from: span.from,
                to: if ends_after { None } else { span.to },
            });
        }
    }
    nw
}

fn marked_spans_after(old: Option<&Spans>, end_ch: i64, is_insert: bool) -> Option<Spans> {
    let mut nw: Option<Spans> = None;
    for span in old.into_iter().flatten() {
        let m = span.marker.data();
        let ends_after = match span.to {
            None => true,
            Some(t) => {
                if m.opts.inclusive_right {
                    t >= end_ch
                } else {
                    t > end_ch
                }
            }
        };
        if ends_after
            || span.from == Some(end_ch)
                && m.kind == MarkerType::Bookmark
                && (!is_insert || m.opts.insert_left)
        {
            let starts_before = match span.from {
                None => true,
                Some(f) => {
                    if m.opts.inclusive_left {
                        f <= end_ch
                    } else {
                        f < end_ch
                    }
                }
            };
            nw.get_or_insert_with(Vec::new).push(MarkedSpan {
                marker: span.marker.clone(),
                from: if starts_before {
                    None
                } else {
                    span.from.map(|f| f - end_ch)
                },
                to: span.to.map(|t| t - end_ch),
            });
        }
    }
    nw
}

pub(crate) type NewSpans = Vec<Option<SpanArr>>;

/// `stretchSpansOverChange(doc, change)`: the span arrays for each line of the
/// changed region after the change.
pub(crate) fn stretch_spans_over_change(doc: &Doc, change: &Change) -> Option<NewSpans> {
    if change.full {
        return None;
    }
    let spans_of = |n: i64| -> Option<Spans> {
        if doc.is_line(n) {
            doc.get_line_ref(n)
                .borrow()
                .marked_spans
                .as_ref()
                .map(|a| a.get())
        } else {
            None
        }
    };
    let old_first = spans_of(change.from.line);
    let old_last = spans_of(change.to.line);
    if old_first.is_none() && old_last.is_none() {
        return None;
    }
    let start_ch = change.from.ch;
    let end_ch = change.to.ch;
    let is_insert = cmp(&change.from, &change.to).is_eq();
    // Get the spans that 'stick out' on both sides.
    let mut first = marked_spans_before(old_first.as_ref(), start_ch, is_insert);
    let mut last = marked_spans_after(old_last.as_ref(), end_ch, is_insert);

    // Next, merge those two ends.
    let text = change.text.as_ref().expect("change with text");
    let same_line = text.len() == 1;
    let offset = text
        .last()
        .map(|t| t.len() as i64)
        .expect("JS TypeError: lst(change.text) is undefined")
        + if same_line { start_ch } else { 0 };
    if let Some(first) = first.as_mut() {
        // Fix up .to properties of first.
        for span in first.iter_mut() {
            if span.to.is_none() {
                match find_in(last.as_ref(), &span.marker) {
                    None => span.to = Some(start_ch),
                    Some(found) => {
                        if same_line {
                            span.to = found.to.map(|t| t + offset);
                        }
                    }
                }
            }
        }
    }
    if let Some(last_v) = last.as_mut() {
        // Fix up .from in last (or move them into first in case of sameLine).
        for span in last_v.iter_mut() {
            if let Some(t) = span.to.as_mut() {
                *t += offset;
            }
            match span.from {
                None => {
                    let found = first
                        .as_ref()
                        .is_some_and(|f| f.iter().any(|s| s.marker.ptr_eq(&span.marker)));
                    if !found {
                        span.from = Some(offset);
                        if same_line {
                            first.get_or_insert_with(Vec::new).push(span.clone());
                        }
                    }
                }
                Some(f) => {
                    span.from = Some(f + offset);
                    if same_line {
                        first.get_or_insert_with(Vec::new).push(span.clone());
                    }
                }
            }
        }
    }
    // Make sure we didn't create any zero-length spans.
    let first = first.and_then(clear_empty_spans);
    // `last != first` is always true here: they are different arrays.
    let last = last.and_then(clear_empty_spans);

    let first_arr = first.clone().map(SpanArr::new);
    let mut new_markers = vec![first_arr];
    if !same_line {
        // Fill gap with whole-line-spans.
        let gap = text.len() as i64 - 2;
        let mut gap_markers: Option<Spans> = None;
        if gap > 0 {
            if let Some(first) = &first {
                for s in first {
                    if s.to.is_none() {
                        gap_markers.get_or_insert_with(Vec::new).push(MarkedSpan {
                            marker: s.marker.clone(),
                            from: None,
                            to: None,
                        });
                    }
                }
            }
        }
        // One array object shared by every gap line, as in the JS.
        let gap_arr = gap_markers.map(SpanArr::new);
        for _ in 0..gap.max(0) {
            new_markers.push(gap_arr.clone());
        }
        new_markers.push(last.map(SpanArr::new));
    }
    Some(new_markers)
}

/// `clearEmptySpans(spans)`.
fn clear_empty_spans(mut spans: Spans) -> Option<Spans> {
    spans.retain(|s| {
        !(s.from.is_some()
            && s.from == s.to
            && s.marker.data().opts.clear_when_empty != Some(false))
    });
    if spans.is_empty() { None } else { Some(spans) }
}

/// `removeReadOnlyRanges(doc, from, to)`: the parts of `from..to` outside read-only
/// markers, or `None` when no read-only marker touches the range.
pub(crate) fn remove_read_only_ranges(doc: &Doc, from: Pos, to: Pos) -> Option<Vec<(Pos, Pos)>> {
    let mut markers: Vec<TextMarker> = Vec::new();
    for line in doc.iter_lines(from.line, to.line + 1) {
        if let Some(spans) = line.borrow().marked_spans.as_ref().map(|a| a.get()) {
            for s in &spans {
                if s.marker.data().opts.read_only && !markers.iter().any(|m| m.ptr_eq(&s.marker)) {
                    markers.push(s.marker.clone());
                }
            }
        }
    }
    if markers.is_empty() {
        return None;
    }
    let mut parts: Vec<(Pos, Pos)> = vec![(from, to)];
    for mk in &markers {
        // `m = mk.find(0)` may be undefined, or lack `to`; the JS only throws when the
        // loop actually reads the missing property, so evaluate lazily.
        let found = mk.find_range_raw();
        let (il, ir) = {
            let d = mk.data();
            (d.opts.inclusive_left, d.opts.inclusive_right)
        };
        let undefined = || -> ! {
            panic!("JS TypeError: Cannot read properties of undefined (reading 'line')")
        };
        let mut j: i64 = 0;
        while (j as usize) < parts.len() {
            let p = parts[j as usize];
            let Some((mfrom, mto)) = found else {
                panic!("JS TypeError: Cannot read properties of undefined (reading 'from')");
            };
            if cmp_num(&p.1, &mfrom) < 0 {
                j += 1;
                continue;
            }
            let mto = mto.unwrap_or_else(|| undefined());
            if cmp_num(&p.0, &mto) > 0 {
                j += 1;
                continue;
            }
            let mut new_parts = Vec::new();
            let dfrom = cmp_num(&p.0, &mfrom);
            let dto = cmp_num(&p.1, &mto);
            if dfrom < 0 || !il && dfrom == 0 {
                new_parts.push((p.0, mfrom));
            }
            if dto > 0 || !ir && dto == 0 {
                new_parts.push((mto, p.1));
            }
            let n = new_parts.len() as i64;
            parts.splice(j as usize..j as usize + 1, new_parts);
            j += n - 1;
            j += 1;
        }
    }
    Some(parts)
}

/// `detachMarkedSpans(line)`.
pub(crate) fn detach_marked_spans(line: &LineRef) {
    let spans = line.borrow().marked_spans.as_ref().map(|a| a.get());
    let Some(spans) = spans else { return };
    for s in &spans {
        s.marker.detach_line(line);
    }
    line.borrow_mut().marked_spans = None;
}

/// `attachMarkedSpans(line, spans)`.
pub(crate) fn attach_marked_spans(line: &LineRef, spans: Option<SpanArr>) {
    let Some(spans) = spans else { return };
    for s in &spans.get() {
        s.marker.attach_line(line);
    }
    line.borrow_mut().marked_spans = Some(spans);
}

fn extra_left(m: &TextMarker) -> i64 {
    if m.data().opts.inclusive_left { -1 } else { 0 }
}
fn extra_right(m: &TextMarker) -> i64 {
    if m.data().opts.inclusive_right { 1 } else { 0 }
}

/// `compareCollapsedMarkers(a, b)`.
pub(crate) fn compare_collapsed_markers(a: &TextMarker, b: &TextMarker) -> i64 {
    let len_diff = a.data().lines.len() as i64 - b.data().lines.len() as i64;
    if len_diff != 0 {
        return len_diff;
    }
    let (Some((af, at)), Some((bf, bt))) = (a.find_range(), b.find_range()) else {
        return 0;
    };
    let mut from_cmp = cmp_num(&af, &bf);
    if from_cmp == 0 {
        from_cmp = extra_left(a) - extra_left(b);
    }
    if from_cmp != 0 {
        return -from_cmp;
    }
    let mut to_cmp = cmp_num(&at, &bt);
    if to_cmp == 0 {
        to_cmp = extra_right(a) - extra_right(b);
    }
    if to_cmp != 0 {
        return to_cmp;
    }
    b.data().id as i64 - a.data().id as i64
}

fn collapsed_span_at_side(line: &LineRef, start: bool) -> Option<TextMarker> {
    let spans = line.borrow().marked_spans.as_ref()?.get();
    let mut found: Option<TextMarker> = None;
    for sp in &spans {
        let side_null = if start {
            sp.from.is_none()
        } else {
            sp.to.is_none()
        };
        if sp.marker.data().collapsed
            && side_null
            && found
                .as_ref()
                .is_none_or(|f| compare_collapsed_markers(f, &sp.marker) < 0)
        {
            found = Some(sp.marker.clone());
        }
    }
    found
}

pub(crate) fn collapsed_span_at_start(line: &LineRef) -> Option<TextMarker> {
    collapsed_span_at_side(line, true)
}
pub(crate) fn collapsed_span_at_end(line: &LineRef) -> Option<TextMarker> {
    collapsed_span_at_side(line, false)
}

/// `collapsedSpanAround(line, ch)`.
// Ported for completeness; only the (unported) view layer calls it.
#[allow(dead_code)]
pub(crate) fn collapsed_span_around(line: &LineRef, ch: i64) -> Option<TextMarker> {
    let spans = line.borrow().marked_spans.as_ref()?.get();
    let mut found: Option<TextMarker> = None;
    for sp in &spans {
        if sp.marker.data().collapsed
            && sp.from.is_none_or(|f| f < ch)
            && sp.to.is_none_or(|t| t > ch)
            && found
                .as_ref()
                .is_none_or(|f| compare_collapsed_markers(f, &sp.marker) < 0)
        {
            found = Some(sp.marker.clone());
        }
    }
    found
}

/// `conflictingCollapsedRange(doc, lineNo, from, to, marker)`; `marker` here is only
/// consulted for its inclusiveLeft/Right flags.
pub(crate) fn conflicting_collapsed_range(
    doc: &Doc,
    line_no: i64,
    from: &Pos,
    to: &Pos,
    il: bool,
    ir: bool,
) -> bool {
    let line = doc.get_line_ref(line_no);
    let Some(spans) = line.borrow().marked_spans.as_ref().map(|a| a.get()) else {
        return false;
    };
    let m_el = if il { -1 } else { 0 };
    let m_er = if ir { 1 } else { 0 };
    for sp in &spans {
        if !sp.marker.data().collapsed {
            continue;
        }
        let Some((ffrom, fto)) = sp.marker.find_range() else {
            continue;
        };
        let mut from_cmp = cmp_num(&ffrom, from);
        if from_cmp == 0 {
            from_cmp = extra_left(&sp.marker) - m_el;
        }
        let mut to_cmp = cmp_num(&fto, to);
        if to_cmp == 0 {
            to_cmp = extra_right(&sp.marker) - m_er;
        }
        if from_cmp >= 0 && to_cmp <= 0 || from_cmp <= 0 && to_cmp >= 0 {
            continue;
        }
        let sp_ir = sp.marker.data().opts.inclusive_right;
        if from_cmp <= 0
            && (if sp_ir && il {
                cmp_num(&fto, from) >= 0
            } else {
                cmp_num(&fto, from) > 0
            })
            || from_cmp >= 0
                && (if sp_ir && il {
                    cmp_num(&ffrom, to) <= 0
                } else {
                    cmp_num(&ffrom, to) < 0
                })
        {
            return true;
        }
    }
    false
}

fn find_line(m: &TextMarker, side: i32) -> Option<(LineRef, i64)> {
    match m.find_lines(side) {
        FindL::Point(l, ch) => Some((l, ch)),
        _ => None,
    }
}

/// `visualLine(line)`.
pub(crate) fn visual_line(mut line: LineRef) -> LineRef {
    while let Some(merged) = collapsed_span_at_start(&line) {
        match find_line(&merged, -1) {
            Some((l, _)) => line = l,
            None => break,
        }
    }
    line
}

/// `visualLineEnd(line)`.
// Ported for completeness; only the (unported) view layer calls it.
#[allow(dead_code)]
pub(crate) fn visual_line_end(mut line: LineRef) -> LineRef {
    while let Some(merged) = collapsed_span_at_end(&line) {
        match find_line(&merged, 1) {
            Some((l, _)) => line = l,
            None => break,
        }
    }
    line
}

/// `visualLineContinued(line)`.
// Ported for completeness; only the (unported) view layer calls it.
#[allow(dead_code)]
pub(crate) fn visual_line_continued(mut line: LineRef) -> Option<Vec<LineRef>> {
    let mut lines: Option<Vec<LineRef>> = None;
    while let Some(merged) = collapsed_span_at_end(&line) {
        match find_line(&merged, 1) {
            Some((l, _)) => {
                line = l;
                lines.get_or_insert_with(Vec::new).push(line.clone());
            }
            None => break,
        }
    }
    lines
}

/// `visualLineNo(doc, lineN)`.
pub(crate) fn visual_line_no(doc: &Doc, line_n: i64) -> i64 {
    let line = doc.get_line_ref(line_n);
    let vis = visual_line(line.clone());
    if Rc::ptr_eq(&line, &vis) {
        return line_n;
    }
    doc.line_no(&vis).unwrap_or(line_n)
}

/// `visualLineEndNo(doc, lineN)`.
pub(crate) fn visual_line_end_no(doc: &Doc, line_n: i64) -> i64 {
    if line_n > doc.last_line() {
        return line_n;
    }
    let mut line = doc.get_line_ref(line_n);
    if !line_is_hidden(doc, &line) {
        return line_n;
    }
    while let Some(merged) = collapsed_span_at_end(&line) {
        match find_line(&merged, 1) {
            Some((l, _)) => line = l,
            None => break,
        }
    }
    doc.line_no(&line).unwrap_or(line_n) + 1
}

/// `lineIsHidden(doc, line)`. No widget nodes exist without a view, so the
/// `widgetNode` checks are always false.
pub(crate) fn line_is_hidden(doc: &Doc, line: &LineRef) -> bool {
    let Some(spans) = line.borrow().marked_spans.as_ref().map(|a| a.get()) else {
        return false;
    };
    for sp in &spans {
        if !sp.marker.data().collapsed {
            continue;
        }
        if sp.from.is_none() {
            return true;
        }
        if sp.from == Some(0)
            && sp.marker.data().opts.inclusive_left
            && line_is_hidden_inner(doc, line, sp)
        {
            return true;
        }
    }
    false
}

fn line_is_hidden_inner(doc: &Doc, line: &LineRef, span: &MarkedSpan) -> bool {
    let Some(span_to) = span.to else {
        let Some((end_line, _)) = find_line(&span.marker, 1) else {
            return false;
        };
        let sp = get_marked_span_for(end_line.borrow().marked_spans.as_ref(), &span.marker);
        return match sp {
            Some(sp) => line_is_hidden_inner(doc, &end_line, &sp),
            None => false,
        };
    };
    if span.marker.data().opts.inclusive_right && span_to == line.borrow().text.len() as i64 {
        return true;
    }
    let spans = line
        .borrow()
        .marked_spans
        .as_ref()
        .map(|a| a.get())
        .unwrap_or_default();
    for sp in &spans {
        let m = sp.marker.data();
        if m.collapsed
            && sp.from == Some(span_to)
            && (sp.to.is_none() || sp.to != span.from)
            && (m.opts.inclusive_left || span.marker.data().opts.inclusive_right)
        {
            drop(m);
            if line_is_hidden_inner(doc, line, sp) {
                return true;
            }
        }
    }
    false
}

/// `heightAtLine(lineObj)`.
pub(crate) fn height_at_line(doc: &Doc, line: LineRef) -> f64 {
    let line = visual_line(line);
    doc.inner().tree.height_above(&line)
}

/// `lineLength(line)`: character length taking collapsed ranges into account.
pub(crate) fn line_length(line: &LineRef) -> i64 {
    if line.borrow().height == 0.0 {
        return 0;
    }
    let mut len = line.borrow().text.len() as i64;
    let mut cur = line.clone();
    while let Some(merged) = collapsed_span_at_start(&cur) {
        let FindL::Range(fl, fch, Some((_, tch))) = merged.find_lines(0) else {
            break;
        };
        cur = fl;
        len += fch - tch;
    }
    cur = line.clone();
    while let Some(merged) = collapsed_span_at_end(&cur) {
        let FindL::Range(_, fch, Some((tl, tch))) = merged.find_lines(0) else {
            break;
        };
        len -= cur.borrow().text.len() as i64 - fch;
        cur = tl;
        len += cur.borrow().text.len() as i64 - tch;
    }
    len
}

#[cfg(test)]
mod tests {
    //! `collapsedSpanAround`, `visualLineEnd` and `visualLineContinued`
    //! (`src/line/spans.js:229`, `:269`, `:278`) have no caller in the ported code. The
    //! expected values come from the JS and were confirmed by running those functions
    //! on an upstream `Doc` in node.
    use super::*;
    use crate::model::mark_text::{MarkOptions, Marker};

    fn fold(doc: &Doc, from: (i64, i64), to: (i64, i64)) -> Marker {
        doc.mark_text(
            Pos::new(from.0, from.1),
            Pos::new(to.0, to.1),
            MarkOptions {
                collapsed: true,
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn no(doc: &Doc, line: &LineRef) -> i64 {
        doc.line_no(line).unwrap()
    }

    /// `sp.from < ch && sp.to > ch`, `null` meaning unbounded; non-collapsed markers
    /// are skipped.
    #[test]
    fn collapsed_span_around_is_strictly_inside() {
        let doc = Doc::new("abcdef\nghij\nklmn\nopq");
        let m = fold(&doc, (0, 3), (2, 2)).text_marker();
        doc.mark_text(
            Pos::new(0, 0),
            Pos::new(3, 3),
            MarkOptions {
                class_name: Some("x".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let around = |l: i64, ch: i64| collapsed_span_around(&doc.get_line_ref(l), ch);
        // line 0: span {from: 3, to: null}
        assert_eq!(around(0, 3), None);
        assert_eq!(around(0, 4).as_ref(), Some(&m));
        assert_eq!(around(0, 6).as_ref(), Some(&m));
        // line 1: span {from: null, to: null}
        assert_eq!(around(1, 0).as_ref(), Some(&m));
        assert_eq!(around(1, 4).as_ref(), Some(&m));
        // line 2: span {from: null, to: 2}
        assert_eq!(around(2, 1).as_ref(), Some(&m));
        assert_eq!(around(2, 2), None);
        assert_eq!(around(3, 0), None);
    }

    /// Of several collapsed spans around `ch`, the one `compareCollapsedMarkers` ranks
    /// highest wins: the enclosing marker.
    #[test]
    fn collapsed_span_around_prefers_outer_marker() {
        let doc = Doc::new("abcdefgh");
        let _inner = fold(&doc, (0, 2), (0, 4));
        let outer = fold(&doc, (0, 1), (0, 6)).text_marker();
        let around = |ch: i64| collapsed_span_around(&doc.get_line_ref(0), ch);
        assert_eq!(around(0), None);
        assert_eq!(around(1), None);
        for ch in 2..=5 {
            assert_eq!(around(ch).as_ref(), Some(&outer), "ch {ch}");
        }
        assert_eq!(around(6), None);

        // More lines wins first (`a.lines.length - b.lines.length`).
        let doc = Doc::new("abcdef\nghij\nklmn");
        let _one = fold(&doc, (1, 1), (1, 3));
        let big = fold(&doc, (0, 2), (2, 1)).text_marker();
        for ch in [0, 2, 4] {
            assert_eq!(
                collapsed_span_around(&doc.get_line_ref(1), ch).as_ref(),
                Some(&big),
                "ch {ch}"
            );
        }
    }

    /// `visualLineEnd` follows collapsed spans at the end of the line to the line
    /// the marker ends on; `visualLineContinued` lists those lines, or is
    /// `undefined` (`None`) when there are none. Intermediate lines fully inside one
    /// marker are not listed.
    #[test]
    fn visual_line_end_and_continued_single_fold() {
        let doc = Doc::new("abcdef\nghij\nklmn\nopq");
        fold(&doc, (0, 3), (2, 2));
        let end = |l: i64| no(&doc, &visual_line_end(doc.get_line_ref(l)));
        let cont = |l: i64| {
            visual_line_continued(doc.get_line_ref(l))
                .map(|v| v.iter().map(|x| no(&doc, x)).collect::<Vec<_>>())
        };
        assert_eq!([end(0), end(1), end(2), end(3)], [2, 2, 2, 3]);
        assert_eq!(cont(0), Some(vec![2]));
        assert_eq!(cont(1), Some(vec![2]));
        assert_eq!(cont(2), None);
        assert_eq!(cont(3), None);
    }

    #[test]
    fn visual_line_end_and_continued_chained_folds() {
        let doc = Doc::new("abcdef\nghij\nklmn\nopq");
        fold(&doc, (0, 3), (1, 2));
        fold(&doc, (1, 3), (2, 1));
        let end = |l: i64| no(&doc, &visual_line_end(doc.get_line_ref(l)));
        let cont = |l: i64| {
            visual_line_continued(doc.get_line_ref(l))
                .map(|v| v.iter().map(|x| no(&doc, x)).collect::<Vec<_>>())
        };
        assert_eq!([end(0), end(1), end(2), end(3)], [2, 2, 2, 3]);
        assert_eq!(cont(0), Some(vec![1, 2]));
        assert_eq!(cont(1), Some(vec![2]));
        assert_eq!(cont(2), None);
        assert_eq!(cont(3), None);

        // Two folds meeting at (1, 2): line 1 is reached through the first and left
        // through the second.
        let doc = Doc::new("abcdef\nghij\nklmn\nopq");
        fold(&doc, (0, 3), (1, 2));
        fold(&doc, (1, 2), (3, 1));
        let end = |l: i64| no(&doc, &visual_line_end(doc.get_line_ref(l)));
        let cont = |l: i64| {
            visual_line_continued(doc.get_line_ref(l))
                .map(|v| v.iter().map(|x| no(&doc, x)).collect::<Vec<_>>())
        };
        assert_eq!([end(0), end(1), end(2), end(3)], [3, 3, 3, 3]);
        assert_eq!(cont(0), Some(vec![1, 3]));
        assert_eq!(cont(1), Some(vec![3]));
        assert_eq!(cont(2), Some(vec![3]));
        assert_eq!(cont(3), None);
    }
}
