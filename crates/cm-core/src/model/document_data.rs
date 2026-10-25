//! `src/model/document_data.js`: `updateDoc`, `isWholeLineUpdate`. (`linkedDocs` is
//! [`Doc::linked_docs`]; `attachDoc` and direction classes belong to the editor.)

use super::changes::Change;
use super::doc::Doc;
use super::line::{LineRef, new_line};
use super::spans::{NewSpans, SpanArr, attach_marked_spans, detach_marked_spans};
use crate::text::Text;

/// `isWholeLineUpdate(doc, change)`: updates that start and end at line starts are
/// treated specially so line objects (and their markers) move intuitively.
pub(crate) fn is_whole_line_update(doc: &Doc, change: &Change) -> bool {
    change.from.ch == 0
        && change.to.ch == 0
        && change.text().last().is_some_and(|t| t.is_empty())
        && doc.whole_line_update_before()
}

/// `updateLine(line, text, markedSpans)` (height estimate is 1 without a view).
fn update_line(doc: &Doc, line: &LineRef, text: Text, spans: Option<SpanArr>) {
    {
        let mut l = line.borrow_mut();
        l.text = text;
        // `if (line.stateAfter) line.stateAfter = null; if (line.styles) line.styles = null`
        l.state_after = None;
        l.styles = None;
    }
    detach_marked_spans(line);
    attach_marked_spans(line, spans);
    if line.borrow().height != 1.0 {
        doc.inner_mut().tree.update_line_height(line, 1.0);
    }
}

/// `updateDoc(doc, change, markedSpans)`.
pub(crate) fn update_doc(doc: &Doc, change: &Change, marked_spans: Option<NewSpans>) {
    let spans_for = |n: usize| -> Option<SpanArr> {
        marked_spans
            .as_ref()
            .and_then(|m| m.get(n).cloned().flatten())
    };
    let text = change.text();
    let lines_for = |start: i64, end: i64| -> Vec<LineRef> {
        (start..end.max(start))
            .map(|i| new_line(text[i as usize].clone(), spans_for(i as usize)))
            .collect()
    };
    // An empty `text` array (e.g. `linkedDoc` with `from >= to`) makes the JS
    // concatenate `undefined` into the line text. Kept for fidelity.
    let undefined: Text = crate::text::to_u16("undefined");
    let tlen = text.len() as i64;
    let text0 = text.first().cloned().unwrap_or_else(|| undefined.clone());

    let from = change.from;
    let to = change.to;
    let first_line = doc.get_line_ref(from.line);
    let last_line = doc.get_line_ref(to.line);
    let last_text = text.last().cloned().unwrap_or_else(|| undefined.clone());
    let last_spans = spans_for(text.len().wrapping_sub(1));
    let nlines = to.line - from.line;
    let slice = |l: &LineRef, a: i64, b: Option<i64>| {
        crate::text::js_slice(&l.borrow().text, a, b).to_vec()
    };
    let concat = |parts: &[&[u16]]| -> Text { parts.concat() };

    // Adjust the line structure.
    if change.full {
        // `doc.insert(0, ..)` / `doc.remove(text.length, ..)` take *absolute* line
        // numbers, so for a document whose first line is not 0 these land in the
        // wrong place (or throw). Kept for fidelity.
        doc.insert_lines(0, lines_for(0, tlen));
        let size = doc.size();
        doc.remove_lines(text.len() as i64, size - text.len());
    } else if is_whole_line_update(doc, change) {
        // A whole-line replace: make sure line objects move the way they should.
        let added = lines_for(0, tlen - 1);
        let lt = last_line.borrow().text.clone();
        update_line(doc, &last_line, lt, last_spans);
        if nlines > 0 {
            doc.remove_lines(from.line, nlines as usize);
        }
        if !added.is_empty() {
            doc.insert_lines(from.line, added);
        }
    } else if std::rc::Rc::ptr_eq(&first_line, &last_line) {
        if text.len() == 1 {
            let t = concat(&[
                &slice(&first_line, 0, Some(from.ch)),
                &last_text,
                &slice(&first_line, to.ch, None),
            ]);
            update_line(doc, &first_line, t, last_spans);
        } else {
            let mut added = lines_for(1, tlen - 1);
            added.push(new_line(
                concat(&[&last_text, &slice(&first_line, to.ch, None)]),
                last_spans,
            ));
            let t = concat(&[&slice(&first_line, 0, Some(from.ch)), &text0]);
            update_line(doc, &first_line, t, spans_for(0));
            doc.insert_lines(from.line + 1, added);
        }
    } else if text.len() == 1 {
        let t = concat(&[
            &slice(&first_line, 0, Some(from.ch)),
            &text0,
            &slice(&last_line, to.ch, None),
        ]);
        update_line(doc, &first_line, t, spans_for(0));
        doc.remove_lines(from.line + 1, nlines as usize);
    } else {
        let t = concat(&[&slice(&first_line, 0, Some(from.ch)), &text0]);
        update_line(doc, &first_line, t, spans_for(0));
        let t = concat(&[&last_text, &slice(&last_line, to.ch, None)]);
        update_line(doc, &last_line, t, last_spans);
        let added = lines_for(1, tlen - 1);
        if nlines > 1 {
            doc.remove_lines(from.line + 1, nlines as usize - 1);
        }
        doc.insert_lines(from.line + 1, added);
    }
}
