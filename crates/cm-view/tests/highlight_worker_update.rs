//! `highlightWorker` (src/display/highlight_worker.js) on the editor, against
//! `tests/data/cm5-highlight-worker.json` (recorded by `oracle/cm5-highlight-worker.mjs`
//! on CodeMirror 5.65.21 in Chromium; issue #66).
//!
//! The worker restyles lines from the highlight frontier on and, for the lines of the
//! rendered range whose styles differ from the ones drawn on demand, calls
//! `regLineChange` inside `runInOp` (lines 47-52), so the editor redraws them and
//! fires `update`. A document that opens a comment above the viewport is the case
//! where the on-demand styles are wrong: `findStartLine` starts a fresh state a screen
//! above the viewport. The recording scrolls there at once and waits for the worker.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, Metrics};
use serde_json::Value;

fn data() -> Value {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cm5-highlight-worker.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("cm5-highlight-worker.json"))
        .unwrap()
}

fn body(n: usize) -> String {
    vec!["var x = 1;"; n].join("\n")
}

fn replay(name: &str, value: &str, nested: bool) {
    let rec = &data()["cases"][name];
    let mut cm = Editor::new(
        value,
        Spec::from("javascript"),
        EditorOptions::default(),
        Metrics::oracle(),
    );
    cm.set_size(600.0, 300.0);
    let updates = Rc::new(Cell::new(0u64));
    let u = updates.clone();
    cm.on_update(move |_| u.set(u.get() + 1));
    // `cm.scrollTo(null, line * 38)` on the oracle's 38px lines.
    cm.scroll_to(None, Some(rec["line"].as_i64().unwrap() as f64 * 38.0));
    let vp = [cm.display.view_from, cm.display.view_to];
    assert_eq!(
        vp,
        [
            rec["viewport"][0].as_i64().unwrap(),
            rec["viewport"][1].as_i64().unwrap()
        ],
        "{name}: viewport"
    );
    let vis = rec["vis"].as_i64().unwrap();
    let spans = |cm: &Editor| cm.line_styles(vis).spans.len() as u64;
    assert_eq!(
        updates.get(),
        rec["synchronous"]["updates"].as_u64().unwrap(),
        "{name}: updates after the scroll"
    );
    assert_eq!(
        spans(&cm),
        rec["synchronous"]["spans"].as_u64().unwrap(),
        "{name}: spans before the worker"
    );
    if nested {
        // `runInOp` inside an operation joins it: one display update at its end.
        cm.run_op(|cm| while cm.highlight_work(200) {});
    } else {
        while cm.highlight_work(200) {}
    }
    assert_eq!(
        updates.get(),
        rec["final"]["updates"].as_u64().unwrap(),
        "{name}: updates after the worker"
    );
    assert_eq!(
        spans(&cm),
        rec["final"]["spans"].as_u64().unwrap(),
        "{name}: spans after the worker"
    );
}

/// w01: the worker finds the visible lines are inside the comment; CM5 fires a second
/// `update` for them.
#[test]
fn w01_comment_started_above_viewport() {
    replay(
        "w01_comment_started_above_viewport",
        &format!("/*\n{}\n*/\nvar y;", body(3000)),
        false,
    );
}

/// w02: no style differs, so the worker fires nothing.
#[test]
fn w02_no_style_difference() {
    replay("w02_no_style_difference", &body(3000), false);
}

/// w01 with the worker slices run inside one operation of the caller (`runInOp` joins
/// the open operation): the same final `update` count and styles.
#[test]
fn w01_inside_an_open_operation() {
    replay(
        "w01_comment_started_above_viewport",
        &format!("/*\n{}\n*/\nvar y;", body(3000)),
        true,
    );
}
