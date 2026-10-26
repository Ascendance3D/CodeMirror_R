//! CM5 differential cases for linked documents and subviews (`shiftDoc`, ledger row
//! 160; QA finding R1). The cases are `tests/data/linked-subview-cases.json`; the
//! recording `tests/data/cm5-linked-subview.json` comes from
//! `oracle/cm5-linked-subview.mjs` (CodeMirror 5.65.21 in Chromium, the page metrics
//! of `Metrics::oracle()`, 600×300).
//!
//! Every case runs twice:
//! - in one [`EditorSet`], where an operation reaches every linked editor as in
//!   CM5. After every step each editor's rendered range, scroll position, line
//!   count, `first`, visible lines, first visible text, `lineNumChars`,
//!   selections and the markers' positions, and the full event log (order, count,
//!   payloads) must equal CM5's.
//! - with separate editors, where an editor that the operation could not reach
//!   finishes its operation at its next entry (decision D6). Each editor is then
//!   entered once after the step. The states must equal CM5's, and so must each
//!   editor's own events in order; only the interleaving across editors differs.
//!
//! CM5's `scroll` event is not ported and is filtered out. A step whose action
//! throws in CM5 must panic in Rust. No rendered range may lie outside its
//! document (`Display::invalid_views` stays 0).
//!
//! [`KNOWN`] lists the differences that do not come from linked documents, each
//! limited to one editor and named steps: a state of that editor, or a log that is
//! equal once that editor's entries are removed. Every entry must still occur.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use cm_core::{Doc, EditorId, LinkedDocOptions, MarkOptions, Marker, MarkerFind, Pos};
use cm_modes::{Registry, Spec};
use cm_view::{Editor, EditorOptions, EditorSet, Metrics};
use serde_json::Value;

type Log = Rc<RefCell<Vec<String>>>;

const CASES: &str = include_str!("data/linked-subview-cases.json");
const RECORDING: &str = include_str!("data/cm5-linked-subview.json");
const TAGS: [&str; 4] = ["A:", "B:", "C:", "D:"];

/// `(case, steps, editor tag, reason)`.
const KNOWN: &[(&str, &[&str], &str, &str)] = &[
    (
        "c19_detach_and_reattach",
        &["b_attach"],
        "B:",
        "construction start range: a new editor first renders at the default height, \
         so the scroll to 400 stays inside it (no update); CM5 renders at 600x300",
    ),
    (
        "c21_undo_redo",
        &["a_undo", "a_redo", "b_edit", "b_undo", "b_undo_again"],
        "A:",
        "N3 (pre-existing on master): undo/redo does not scroll the cursor into \
         view as makeChangeFromHistory -> ensureCursorVisible does in CM5",
    ),
    (
        "c21b_undo_redo_parent_at_top",
        &[
            "a_undo",
            "a_redo",
            "a_delete_across_first",
            "a_undo_delete",
            "b_undo_insert_above",
            "b_redo",
        ],
        "A:",
        "N3 (pre-existing on master), as in c21_undo_redo",
    ),
];

fn lines(n: usize) -> String {
    (0..n)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}
fn ps(p: Pos) -> String {
    format!("{}:{}", p.line, p.ch)
}
fn pos(v: &Value) -> Pos {
    Pos::new(v[0].as_i64().unwrap(), v[1].as_i64().unwrap())
}
fn watch(cm: &Editor, log: &Log, t: &'static str) {
    let l = log.clone();
    cm.on_change(move |_, c| {
        let text = serde_json::to_string(&c.text).unwrap();
        l.borrow_mut()
            .push(format!("{t}change({},{},{text})", ps(c.from), ps(c.to)));
    });
    let l = log.clone();
    cm.on_changes(move |_, cs| l.borrow_mut().push(format!("{t}changes({})", cs.len())));
    let l = log.clone();
    cm.on_update(move |_| l.borrow_mut().push(format!("{t}update")));
    let l = log.clone();
    cm.on_viewport_change(move |_, a, b| {
        l.borrow_mut().push(format!("{t}viewportChange({a},{b})"))
    });
    let l = log.clone();
    cm.on_cursor_activity(move |_| l.borrow_mut().push(format!("{t}cursorActivity")));
}

#[derive(Debug, PartialEq)]
struct State {
    vp: (i64, i64),
    top: f64,
    left: f64,
    lines: i64,
    first: i64,
    visible: (i64, i64),
    first_text: String,
    line_num_chars: Option<i64>,
    sels: Vec<String>,
}
fn state(cm: &Editor) -> State {
    let top = cm.display.scroll_top - cm.metrics.padding_top;
    let first = cm.line_at_height(top);
    State {
        vp: (cm.display.view_from, cm.display.view_to),
        top: cm.display.scroll_top,
        left: cm.display.scroll_left,
        lines: cm.doc.line_count() as i64,
        first: cm.doc.first_line(),
        visible: (first, cm.line_at_height(top + cm.display.height)),
        first_text: cm.doc.get_line(first).unwrap_or_default(),
        line_num_chars: cm.display.line_num_chars.map(|n| n as i64),
        sels: cm
            .doc
            .list_selections()
            .iter()
            .map(|r| format!("{}-{}", ps(r.anchor), ps(r.head)))
            .collect(),
    }
}
fn rstate(v: &Value) -> Option<State> {
    if v.is_null() {
        return None;
    }
    let pair = |a: &Value| (a[0].as_i64().unwrap(), a[1].as_i64().unwrap());
    Some(State {
        vp: pair(&v["vp"]),
        top: v["top"].as_f64().unwrap(),
        left: v["left"].as_f64().unwrap(),
        lines: v["lines"].as_i64().unwrap(),
        first: v["first"].as_i64().unwrap(),
        visible: pair(&v["visible"]),
        first_text: v["firstText"].as_str().unwrap().to_string(),
        line_num_chars: v["lineNumChars"].as_i64(),
        sels: v["sels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect(),
    })
}
fn rlog(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap().to_string())
        .filter(|e| !e.ends_with("scroll"))
        .collect()
}
fn marker_pos(m: &Marker) -> Option<String> {
    match m.find()? {
        MarkerFind::Range { from, to } => Some(format!("{}-{}", ps(from), ps(to.unwrap_or(from)))),
        MarkerFind::Point(p) => Some(format!("{}-{}", ps(p), ps(p))),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// One `EditorSet`: exact event order across editors.
    Set,
    /// Separate editors (D6): per-editor event order.
    Plain,
}

struct Harness {
    mode: Mode,
    set: EditorSet,
    ids: Vec<Option<EditorId>>,
    plain: Vec<Option<Editor>>,
    docs: Vec<Doc>,
    numbers: Vec<bool>,
    markers: Vec<Marker>,
    log: Log,
}

fn options(numbers: bool) -> EditorOptions {
    EditorOptions {
        line_numbers: numbers,
        ..Default::default()
    }
}

impl Harness {
    fn new(mode: Mode, case: &Value) -> Harness {
        let n = case["lines"].as_u64().unwrap_or(200) as usize;
        let numbers = case["lineNumbers"].as_bool().unwrap_or(false);
        let mut a = Editor::new(&lines(n), Spec::Null, options(numbers), Metrics::oracle());
        a.set_size(600.0, 300.0);
        let mut h = Harness {
            mode,
            set: EditorSet::new(),
            ids: Vec::new(),
            plain: Vec::new(),
            docs: vec![a.doc.clone()],
            numbers: vec![numbers],
            markers: Vec::new(),
            log: Rc::default(),
        };
        h.add(0, a);
        for (k, s) in case["subs"].as_array().unwrap().iter().enumerate() {
            let d = h.docs[0]
                .linked_doc(LinkedDocOptions {
                    shared_hist: true,
                    from: s["from"].as_i64(),
                    to: s["to"].as_i64(),
                    mode: None,
                })
                .unwrap();
            h.docs.push(d);
            h.numbers.push(s["lineNumbers"].as_bool().unwrap_or(false));
            let ed = h.editor_for(k + 1);
            h.add(k + 1, ed);
        }
        h
    }
    fn editor_for(&self, i: usize) -> Editor {
        let mut ed = Editor::with_doc(
            self.docs[i].clone(),
            Spec::Null,
            options(self.numbers[i]),
            Metrics::oracle(),
            Rc::new(Registry::with_all_modes()),
        );
        ed.set_size(600.0, 300.0);
        ed
    }
    fn add(&mut self, i: usize, ed: Editor) {
        watch(&ed, &self.log, TAGS[i]);
        match self.mode {
            Mode::Set => {
                let id = self.set.insert(ed);
                set_slot(&mut self.ids, i, Some(id));
            }
            Mode::Plain => set_slot(&mut self.plain, i, Some(ed)),
        }
    }
    fn get(&self, i: usize) -> Option<&Editor> {
        match self.mode {
            Mode::Set => self
                .ids
                .get(i)
                .copied()
                .flatten()
                .and_then(|id| self.set.get(id)),
            Mode::Plain => self.plain.get(i).and_then(Option::as_ref),
        }
    }
    fn count(&self) -> usize {
        self.docs.len()
    }
    /// `cm.operation`-wrapped work on editor `i`.
    fn run(&mut self, i: usize, f: impl FnOnce(&mut Editor, &mut Vec<Marker>)) {
        let markers = &mut self.markers;
        match self.mode {
            Mode::Set => {
                let id = self.ids[i].unwrap();
                self.set.run(id, |cx| f(cx.editor, markers));
            }
            Mode::Plain => {
                let ed = self.plain[i].as_mut().unwrap();
                ed.run_op(|ed| f(ed, markers));
            }
        }
    }
    fn act(&mut self, a: &Value) {
        let (k, v) = a.as_object().unwrap().iter().next().unwrap();
        match k.as_str() {
            "detach" => {
                let i = v.as_u64().unwrap() as usize;
                match self.mode {
                    Mode::Set => drop(self.set.remove(self.ids[i].take().unwrap())),
                    Mode::Plain => drop(self.plain[i].take()),
                }
            }
            "attach" => {
                let i = v.as_u64().unwrap() as usize;
                let ed = self.editor_for(i);
                self.add(i, ed);
            }
            "unlink" => {
                let (i, j) = (
                    v[0].as_u64().unwrap() as usize,
                    v[1].as_u64().unwrap() as usize,
                );
                self.docs[i].unlink_doc(&self.docs[j]);
            }
            _ => {
                let i = target(a);
                let a = a.clone();
                self.run(i, move |ed, markers| apply(ed, &a, markers));
            }
        }
    }
    /// Separate editors: enter each one once, so that work parked on it is done.
    fn settle(&mut self) {
        if self.mode == Mode::Plain {
            for ed in self.plain.iter_mut().flatten() {
                ed.run_op(|_| ());
            }
        }
    }
}

fn set_slot<T>(v: &mut Vec<Option<T>>, i: usize, x: Option<T>) {
    if v.len() <= i {
        v.resize_with(i + 1, || None);
    }
    v[i] = x;
}
fn target(a: &Value) -> usize {
    let (k, v) = a.as_object().unwrap().iter().next().unwrap();
    match k.as_str() {
        "undo" | "redo" => v.as_u64().unwrap() as usize,
        _ => v[0].as_u64().unwrap() as usize,
    }
}
/// One action inside an operation of editor `ed` (CM5 `cm.<method>`).
fn apply(ed: &mut Editor, a: &Value, markers: &mut Vec<Marker>) {
    let (k, v) = a.as_object().unwrap().iter().next().unwrap();
    match k.as_str() {
        "rr" => {
            let to = if v[3].is_null() {
                None
            } else {
                Some(pos(&v[3]))
            };
            ed.doc
                .replace_range(v[1].as_str().unwrap(), pos(&v[2]), to, None);
        }
        "scroll" => ed.scroll_to(None, Some(v[1].as_f64().unwrap())),
        "op" => {
            let acts = v[1].as_array().unwrap().clone();
            ed.run_op(|ed| {
                for a in &acts {
                    apply(ed, a, markers);
                }
            });
        }
        "undo" => ed.doc.undo(),
        "redo" => ed.doc.redo(),
        "cursor" => ed.doc.set_cursor(pos(&v[1]), None),
        "mark" => markers.push(
            ed.doc
                .mark_text(
                    pos(&v[1]),
                    pos(&v[2]),
                    MarkOptions {
                        class_name: Some("q".into()),
                        ..Default::default()
                    },
                )
                .unwrap(),
        ),
        other => panic!("unknown action {other}"),
    }
}

struct Cmp {
    diffs: Vec<String>,
    /// Differences matched by [`KNOWN`], with the entry's index.
    known: Vec<(usize, String)>,
    compared: usize,
}

impl Cmp {
    fn known_entry(case: &str, step: &str, tag: &str) -> Option<usize> {
        KNOWN
            .iter()
            .position(|(c, steps, t, _)| *c == case && *t == tag && steps.contains(&step))
    }
    fn push(&mut self, known: Option<usize>, d: String) {
        match known {
            Some(k) => self.known.push((k, d)),
            None => self.diffs.push(d),
        }
    }
}

fn run_case(mode: Mode, case: &Value, rec: &Value, cmp: &mut Cmp) {
    let name = case["name"].as_str().unwrap();
    let m = if mode == Mode::Set { "set" } else { "plain" };
    let mut h = Harness::new(mode, case);
    let tops: Vec<f64> = case["tops"]
        .as_array()
        .map(|t| t.iter().map(|x| x.as_f64().unwrap()).collect())
        .unwrap_or(vec![2000.0, 3000.0]);
    for t in tops {
        h.run(0, |ed, _| ed.scroll_to(None, Some(t)));
    }
    for (k, s) in case["subs"].as_array().unwrap().iter().enumerate() {
        for t in s["tops"].as_array().unwrap() {
            let t = t.as_f64().unwrap();
            h.run(k + 1, |ed, _| ed.scroll_to(None, Some(t)));
        }
    }
    h.settle();
    h.log.borrow_mut().clear();
    let mut check = |h: &Harness, label: &str, r: &Value, got_log: &[String], panicked: bool| {
        for i in 0..h.count() {
            cmp.compared += 1;
            let got = h.get(i).map(state);
            let want = rstate(&r["eds"][i]);
            if got != want {
                cmp.push(
                    Cmp::known_entry(name, label, TAGS[i]),
                    format!(
                        "{name} [{m}] {label} {}\n   rust {got:?}\n   cm5  {want:?}",
                        TAGS[i]
                    ),
                );
            }
            if let Some(ed) = h.get(i)
                && ed.display.invalid_views.get() != 0
            {
                cmp.diffs.push(format!(
                    "{name} [{m}] {label} {}: invalid view {:?}",
                    TAGS[i],
                    ed.display.last_invalid_view.get()
                ));
            }
        }
        if !r["markers"].is_null() {
            let got: Vec<Option<String>> = h.markers.iter().map(marker_pos).collect();
            let want: Vec<Option<String>> = r["markers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_str().map(str::to_string))
                .collect();
            if got != want {
                cmp.diffs.push(format!(
                    "{name} [{m}] {label} markers rust {got:?} cm5 {want:?}"
                ));
            }
        }
        if !r["log"].is_null() {
            let want = rlog(&r["log"]);
            let without = |l: &[String], skip: Option<&str>| -> Vec<String> {
                l.iter()
                    .filter(|e| skip.is_none_or(|t| !e.starts_with(t)))
                    .cloned()
                    .collect()
            };
            let same = |skip: Option<&str>| match mode {
                Mode::Set => without(got_log, skip) == without(&want, skip),
                Mode::Plain => TAGS.iter().filter(|t| Some(**t) != skip).all(|t| {
                    let f = |l: &[String]| -> Vec<String> {
                        l.iter().filter(|e| e.starts_with(t)).cloned().collect()
                    };
                    f(got_log) == f(&want)
                }),
            };
            if !same(None) {
                let known = TAGS
                    .iter()
                    .filter_map(|t| Cmp::known_entry(name, label, t).filter(|_| same(Some(t))))
                    .next();
                cmp.push(
                    known,
                    format!("{name} [{m}] {label} log\n   rust {got_log:?}\n   cm5  {want:?}"),
                );
            }
        }
        let threw = r.get("error").is_some();
        if threw != panicked {
            cmp.diffs.push(format!(
                "{name} [{m}] {label}: cm5 threw {threw}, rust panicked {panicked}"
            ));
        }
    };
    check(&h, "before", &rec["before"], &[], false);
    for (si, step) in case["steps"].as_array().unwrap().iter().enumerate() {
        let label = step[0].as_str().unwrap();
        let mut panicked = false;
        for a in step[1].as_array().unwrap() {
            if catch_unwind(AssertUnwindSafe(|| h.act(a))).is_err() {
                panicked = true;
                break;
            }
        }
        h.settle();
        let got = std::mem::take(&mut *h.log.borrow_mut());
        check(&h, label, &rec["steps"][si], &got, panicked);
    }
}

fn run_all(mode: Mode) {
    let cases: Value = serde_json::from_str(CASES).unwrap();
    let rec: Value = serde_json::from_str(RECORDING).unwrap();
    assert_eq!(rec["version"], "5.65.21");
    let mut cmp = Cmp {
        diffs: Vec::new(),
        known: Vec::new(),
        compared: 0,
    };
    let list = cases["cases"].as_array().unwrap();
    for case in list {
        let name = case["name"].as_str().unwrap();
        run_case(mode, case, &rec["cases"][name], &mut cmp);
    }
    println!(
        "{} cases, {} editor states compared, {} mismatches, {} known differences",
        list.len(),
        cmp.compared,
        cmp.diffs.len(),
        cmp.known.len()
    );
    for (k, d) in &cmp.known {
        println!("KNOWN ({}): {d}", KNOWN[*k].3);
    }
    for d in &cmp.diffs {
        println!("{d}");
    }
    assert!(cmp.diffs.is_empty(), "{} mismatches", cmp.diffs.len());
    for (k, (case, ..)) in KNOWN.iter().enumerate() {
        assert!(
            cmp.known.iter().any(|(i, _)| *i == k),
            "known difference {case} no longer occurs: update KNOWN"
        );
    }
}

#[test]
fn linked_subviews_in_one_editor_set_match_cm5() {
    run_all(Mode::Set);
}

#[test]
fn linked_subviews_as_separate_editors_match_cm5_per_editor() {
    run_all(Mode::Plain);
}
