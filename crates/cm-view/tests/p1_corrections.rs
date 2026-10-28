//! CM5 differential cases for the P1 F1–F4 correction lane: the 25 independent QA
//! probes of `2026-10-26-p1-final-qa-5adc052` (p01–p12d), then `regLineChange`
//! (F1, `f1_*`) and `maybeUpdateLineNumberWidth`/`resetView` (F3, `f3_*`) cases.
//! The recording `tests/data/cm5-p1-corrections.json` comes from
//! `oracle/cm5-p1-corrections.mjs` (CodeMirror 5.65.21 in Chromium, same page
//! metrics as the viewport matrix: `Metrics::oracle()`, 600×300).
//!
//! After every step the rendered range, scroll position, line count, visible lines,
//! first visible text, whether the gutter width changed, and the full event log
//! (order, count, payloads) must equal CM5's. CM5's `scroll` event is not ported and
//! is filtered out. Wrapped cases (`geometric`) compare the visible lines by
//! geometry: they must lie inside the rendered range.

use std::cell::RefCell;
use std::rc::Rc;

use cm_core::{LinkedDocOptions, MarkOptions, Marker, Pos};
use cm_modes::{Registry, Spec};
use cm_view::app::{App, AppOptions, Input, MemClipboard};
use cm_view::{Editor, EditorOptions, EditorSet, Metrics};
use serde_json::Value;

type Log = Rc<RefCell<Vec<String>>>;

const RECORDING: &str = include_str!("data/cm5-p1-corrections.json");

fn rec() -> Value {
    let v: Value = serde_json::from_str(RECORDING).unwrap();
    assert_eq!(v["version"], "5.65.21");
    v
}
fn p(line: i64, ch: i64) -> Pos {
    Pos::new(line, ch)
}
fn lines(n: usize) -> String {
    (0..n)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}
fn ps(p: Pos) -> String {
    format!("{}:{}", p.line, p.ch)
}
fn watch(cm: &Editor, log: &Log, tag: &str) {
    let (l, t) = (log.clone(), tag.to_string());
    cm.on_change(move |_, c| {
        let text = serde_json::to_string(&c.text).unwrap();
        l.borrow_mut()
            .push(format!("{t}change({},{},{text})", ps(c.from), ps(c.to)));
    });
    let (l, t) = (log.clone(), tag.to_string());
    cm.on_changes(move |_, cs| l.borrow_mut().push(format!("{t}changes({})", cs.len())));
    let (l, t) = (log.clone(), tag.to_string());
    cm.on_update(move |_| l.borrow_mut().push(format!("{t}update")));
    let (l, t) = (log.clone(), tag.to_string());
    cm.on_viewport_change(move |_, a, b| {
        l.borrow_mut().push(format!("{t}viewportChange({a},{b})"))
    });
}

#[derive(Debug, PartialEq)]
struct State {
    vp: (i64, i64),
    top: f64,
    left: f64,
    lines: i64,
    visible: (i64, i64),
    first_text: String,
}
fn state(cm: &Editor) -> State {
    let top = cm.display.scroll_top - cm.metrics.padding_top;
    let first = cm.line_at_height(top);
    State {
        vp: (cm.display.view_from, cm.display.view_to),
        top: cm.display.scroll_top,
        left: cm.display.scroll_left,
        lines: cm.doc.line_count() as i64,
        visible: (first, cm.line_at_height(top + cm.display.height)),
        first_text: cm.doc.get_line(first).unwrap_or_default(),
    }
}
fn rstate(v: &Value) -> State {
    let pair = |a: &Value| (a[0].as_i64().unwrap(), a[1].as_i64().unwrap());
    State {
        vp: pair(&v["vp"]),
        top: v["top"].as_f64().unwrap(),
        left: v["left"].as_f64().unwrap(),
        lines: v["lines"].as_i64().unwrap(),
        visible: pair(&v["visible"]),
        first_text: v["firstText"].as_str().unwrap().to_string(),
    }
}
fn rlog(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap().to_string())
        .filter(|e| !e.ends_with("scroll"))
        .collect()
}
fn editor_with(text: &str, options: EditorOptions) -> Editor {
    let mut cm = Editor::new(text, Spec::Null, options, Metrics::oracle());
    cm.set_size(600.0, 300.0);
    cm
}
fn rr(cm: &mut Editor, text: &str, from: Pos, to: Option<Pos>) {
    cm.run_op(|cm| cm.doc.replace_range(text, from, to, None));
}
fn rr_in(cm: &mut Editor, text: &str, from: Pos, to: Option<Pos>) {
    cm.doc.replace_range(text, from, to, None);
}
fn mark(cm: &Editor, a: i64, b: i64) -> Marker {
    cm.doc
        .mark_text(
            p(a, 0),
            p(b, 2),
            MarkOptions {
                class_name: Some("q".into()),
                ..Default::default()
            },
        )
        .unwrap()
}
fn long() -> String {
    "w".repeat(70)
}

type Act = Box<dyn Fn(&mut Editor)>;
struct Probe {
    name: &'static str,
    text: String,
    options: EditorOptions,
    top: f64,
    setup: Option<Act>,
    steps: Vec<(&'static str, Act)>,
    geometric: bool,
}
fn probe(name: &'static str, steps: Vec<(&'static str, Act)>) -> Probe {
    Probe {
        name,
        text: lines(200),
        options: EditorOptions::default(),
        top: 3000.0,
        setup: None,
        steps,
        geometric: false,
    }
}
fn numbered(name: &'static str, n: usize, top: f64, steps: Vec<(&'static str, Act)>) -> Probe {
    Probe {
        text: lines(n),
        top,
        options: EditorOptions {
            line_numbers: true,
            ..Default::default()
        },
        ..probe(name, steps)
    }
}
fn wrapped() -> EditorOptions {
    EditorOptions {
        line_wrapping: true,
        ..Default::default()
    }
}
fn b(f: impl Fn(&mut Editor) + 'static) -> Act {
    Box::new(f)
}
fn vf(cm: &Editor) -> i64 {
    cm.display.view_from
}
fn vt(cm: &Editor) -> i64 {
    cm.display.view_to
}
/// Scrolled away first, so that a case starting at the top does not keep the
/// range drawn at the default height (see `viewport_matrix.rs`, s01).
fn away() -> Act {
    b(|cm| cm.scroll_to(None, Some(2000.0)))
}
const UP: f64 = 3000.0 + 4.0 * 38.0;
const DOWN: f64 = 3000.0 - 4.0 * 38.0;
fn scroll(top: f64) -> (&'static str, Act) {
    ("scroll", b(move |cm| cm.scroll_to(None, Some(top))))
}

/// The QA probes p01–p12d (`qa_probes.rs` of the QA report, unchanged actions).
fn qa_probes() -> Vec<Probe> {
    let thirty: String = (0..30).map(|i| format!("n{i}\n")).collect();
    vec![
        probe(
            "p01_multi_insert_above",
            vec![(
                "op",
                b(|cm| {
                    cm.run_op(|cm| {
                        rr_in(cm, "X\n", p(0, 0), None);
                        rr_in(cm, "Y\n", p(10, 0), None);
                        rr_in(cm, "Z\n", p(20, 0), None);
                    })
                }),
            )],
        ),
        probe(
            "p02_multi_delete_above",
            vec![(
                "op",
                b(|cm| {
                    cm.run_op(|cm| {
                        rr_in(cm, "", p(0, 0), Some(p(1, 0)));
                        rr_in(cm, "", p(10, 0), Some(p(11, 0)));
                        rr_in(cm, "", p(20, 0), Some(p(22, 0)));
                    })
                }),
            )],
        ),
        probe(
            "p03a_replace_same_count_cross_top",
            vec![("rr", b(|cm| rr(cm, "a\nb", p(67, 0), Some(p(68, 7)))))],
        ),
        probe(
            "p03b_replace_first_rendered_line",
            vec![("rr", b(|cm| rr(cm, "ZZZZZZZ", p(68, 0), Some(p(68, 7)))))],
        ),
        probe(
            "p04a_insert_30_above",
            vec![("rr", b(move |cm| rr(cm, &thirty, p(0, 0), None)))],
        ),
        probe(
            "p04b_delete_30_above",
            vec![("rr", b(|cm| rr(cm, "", p(0, 0), Some(p(30, 0)))))],
        ),
        numbered(
            "p05a_gutter_grows_above",
            99,
            3000.0,
            vec![("rr", b(|cm| rr(cm, "X\n", p(0, 0), None)))],
        ),
        numbered(
            "p05b_gutter_grows_below",
            99,
            3000.0,
            vec![("rr", b(|cm| rr(cm, "\nX", p(98, 7), None)))],
        ),
        numbered(
            "p05c_gutter_shrinks_above",
            100,
            3000.0,
            vec![("rr", b(|cm| rr(cm, "", p(0, 0), Some(p(1, 0)))))],
        ),
        numbered(
            "p05d_gutter_same_inside",
            200,
            3000.0,
            vec![("rr", b(|cm| rr(cm, "X\n", p(80, 0), None)))],
        ),
        probe(
            "p06a_scroll_then_edit_op",
            vec![(
                "op",
                b(|cm| {
                    cm.run_op(|cm| {
                        cm.scroll_to(None, Some(4000.0));
                        rr_in(cm, "X\n", p(0, 0), None);
                    })
                }),
            )],
        ),
        probe(
            "p06b_edit_inside_then_scroll_op",
            vec![(
                "op",
                b(|cm| {
                    cm.run_op(|cm| {
                        rr_in(cm, "X\n", p(80, 0), None);
                        cm.scroll_to(None, Some(1000.0));
                    })
                }),
            )],
        ),
        probe(
            "p07a_scroll_op_1px",
            vec![("scroll", b(|cm| cm.scroll_to(None, Some(3001.0))))],
        ),
        probe(
            "p07b_scroll_op_half_line",
            vec![("scroll", b(|cm| cm.scroll_to(None, Some(3019.0))))],
        ),
        Probe {
            options: wrapped(),
            geometric: true,
            setup: Some(b(|cm| rr(cm, &long(), p(82, 0), None))),
            ..probe(
                "p10a_wrap_inside_shrinks",
                vec![("rr", b(|cm| rr(cm, "", p(82, 0), Some(p(82, 70)))))],
            )
        },
        Probe {
            options: wrapped(),
            geometric: true,
            ..probe(
                "p10b_wrap_multi_above_grows",
                vec![(
                    "op",
                    b(|cm| {
                        cm.run_op(|cm| {
                            rr_in(cm, &long(), p(0, 0), None);
                            rr_in(cm, &long(), p(1, 0), None);
                            rr_in(cm, &long(), p(2, 0), None);
                        })
                    }),
                )],
            )
        },
        probe(
            "p12a_retained_top_char_edit",
            vec![
                scroll(UP),
                ("char", b(|cm| rr(cm, "X", p(vf(cm), 0), None))),
            ],
        ),
        probe(
            "p12b_retained_bottom_char_edit",
            vec![
                scroll(DOWN),
                ("char", b(|cm| rr(cm, "X", p(vt(cm) - 1, 0), None))),
            ],
        ),
        probe(
            "p12c_retained_top_mark_class",
            vec![
                scroll(UP),
                (
                    "mark",
                    b(|cm| {
                        let f = vf(cm);
                        cm.run_op(|cm| {
                            mark(cm, f, f);
                        })
                    }),
                ),
            ],
        ),
        probe(
            "p12d_retained_top_insert_line",
            vec![
                scroll(UP),
                ("line", b(|cm| rr(cm, "X\n", p(vf(cm), 0), None))),
            ],
        ),
    ]
}

/// F1: `regLineChange` marks a rendered line dirty and never cuts the range.
fn f1_probes() -> Vec<Probe> {
    let held: Rc<RefCell<Option<Marker>>> = Rc::default();
    let (h1, h2) = (held.clone(), held);
    vec![
        probe(
            "f1_first_line_no_retention",
            vec![("char", b(|cm| rr(cm, "X", p(vf(cm), 0), None)))],
        ),
        probe(
            "f1_last_line_no_retention",
            vec![("char", b(|cm| rr(cm, "X", p(vt(cm) - 1, 0), None)))],
        ),
        probe(
            "f1_scroll_up1_first_line",
            vec![
                scroll(3000.0 + 38.0),
                ("char", b(|cm| rr(cm, "X", p(vf(cm), 0), None))),
            ],
        ),
        probe(
            "f1_scroll_down1_last_line",
            vec![
                scroll(3000.0 - 38.0),
                ("char", b(|cm| rr(cm, "X", p(vt(cm) - 1, 0), None))),
            ],
        ),
        probe(
            "f1_scroll_up4_last_line",
            vec![
                scroll(UP),
                ("char", b(|cm| rr(cm, "X", p(vt(cm) - 1, 0), None))),
            ],
        ),
        probe(
            "f1_scroll_down4_first_line",
            vec![
                scroll(DOWN),
                ("char", b(|cm| rr(cm, "X", p(vf(cm), 0), None))),
            ],
        ),
        probe(
            "f1_multi_line_edits_one_op",
            vec![
                scroll(UP),
                (
                    "op",
                    b(|cm| {
                        cm.run_op(|cm| {
                            let (a, z) = (vf(cm), vt(cm));
                            rr_in(cm, "X", p(a, 0), None);
                            rr_in(cm, "Y", p(z - 1, 0), None);
                            rr_in(cm, "Z", p(a + 10, 0), None);
                        })
                    }),
                ),
            ],
        ),
        probe(
            "f1_marker_multi_lines",
            vec![
                scroll(UP),
                (
                    "mark",
                    b(|cm| {
                        cm.run_op(|cm| {
                            mark(cm, vf(cm), vf(cm) + 2);
                        })
                    }),
                ),
            ],
        ),
        probe(
            "f1_marker_across_top",
            vec![
                scroll(UP),
                (
                    "mark",
                    b(|cm| {
                        cm.run_op(|cm| {
                            mark(cm, vf(cm) - 2, vf(cm) + 1);
                        })
                    }),
                ),
            ],
        ),
        probe(
            "f1_marker_add_remove_one_op",
            vec![
                scroll(UP),
                (
                    "op",
                    b(|cm| cm.run_op(|cm| mark(cm, vf(cm), vf(cm)).clear())),
                ),
            ],
        ),
        probe(
            "f1_marker_clear_first_line",
            vec![
                scroll(UP),
                (
                    "mark",
                    b(move |cm| {
                        let m = cm.run_op(|cm| mark(cm, vf(cm), vf(cm)));
                        *h1.borrow_mut() = Some(m);
                    }),
                ),
                scroll(UP + 2.0 * 38.0),
                (
                    "clear",
                    b(move |cm| {
                        let m = h2.borrow_mut().take().unwrap();
                        cm.run_op(|_| m.clear());
                    }),
                ),
            ],
        ),
        probe(
            "f1_line_edit_then_insert_one_op",
            vec![
                scroll(UP),
                (
                    "op",
                    b(|cm| {
                        cm.run_op(|cm| {
                            rr_in(cm, "X", p(vf(cm), 0), None);
                            rr_in(cm, "N\n", p(0, 0), None);
                        })
                    }),
                ),
            ],
        ),
        probe(
            "f1_insert_then_line_style_one_op",
            vec![
                scroll(UP),
                (
                    "op",
                    b(|cm| {
                        cm.run_op(|cm| {
                            let a = vf(cm);
                            rr_in(cm, "N\n", p(0, 0), None);
                            mark(cm, a + 1, a + 1);
                        })
                    }),
                ),
            ],
        ),
        probe(
            "f1_insert_inside_then_line_edit_one_op",
            vec![
                scroll(UP),
                (
                    "op",
                    b(|cm| {
                        cm.run_op(|cm| {
                            let a = vf(cm);
                            rr_in(cm, "N\n", p(a + 5, 0), None);
                            rr_in(cm, "X", p(a, 0), None);
                        })
                    }),
                ),
            ],
        ),
        probe(
            "f1_whole_line_update_first_line",
            vec![
                scroll(UP),
                ("empty", b(|cm| rr(cm, "", p(vf(cm), 0), None))),
            ],
        ),
        Probe {
            top: 0.0,
            setup: Some(away()),
            ..probe(
                "f1_set_value_at_top",
                vec![("set", b(|cm| cm.set_value(&lines(200))))],
            )
        },
    ]
}

/// F3: the gutter width is checked after the first bail-out, and a new number of
/// digits resets the view.
fn f3_probes() -> Vec<Probe> {
    let above = || -> Act { b(|cm| rr(cm, "X\n", p(0, 0), None)) };
    let del_above = || -> Act { b(|cm| rr(cm, "", p(0, 0), Some(p(1, 0)))) };
    let first_number = |name, n, first, top| Probe {
        options: EditorOptions {
            line_numbers: true,
            first_line_number: first,
            ..Default::default()
        },
        ..numbered(name, n, top, vec![("rr", above())])
    };
    vec![
        numbered("f3_9_to_10_above", 9, 0.0, vec![("rr", above())]),
        numbered("f3_10_to_9_above", 10, 0.0, vec![("rr", del_above())]),
        numbered(
            "f3_9_to_10_below",
            9,
            0.0,
            vec![("rr", b(|cm| rr(cm, "\nX", p(8, 6), None)))],
        ),
        numbered("f3_999_to_1000_above", 999, 3000.0, vec![("rr", above())]),
        numbered(
            "f3_1000_to_999_above",
            1000,
            3000.0,
            vec![("rr", del_above())],
        ),
        numbered(
            "f3_999_to_1000_below",
            999,
            3000.0,
            vec![("rr", b(|cm| rr(cm, "\nX", p(998, 8), None)))],
        ),
        Probe {
            setup: Some(away()),
            ..numbered("f3_99_to_100_unscrolled", 99, 0.0, vec![("rr", above())])
        },
        Probe {
            setup: Some(away()),
            ..numbered(
                "f3_99_to_100_inside_unscrolled",
                99,
                0.0,
                vec![("rr", b(|cm| rr(cm, "X\n", p(3, 0), None)))],
            )
        },
        Probe {
            text: lines(99),
            ..probe("f3_line_numbers_off", vec![("rr", above())])
        },
        first_number("f3_first_line_number_large", 10, 99990, 0.0),
        first_number("f3_first_line_number_large_scrolled", 98, 9902, 3000.0),
        numbered(
            "f3_nested_op",
            99,
            3000.0,
            vec![(
                "op",
                b(|cm| cm.run_op(|cm| cm.run_op(|cm| rr_in(cm, "X\n", p(0, 0), None)))),
            )],
        ),
        numbered(
            "f3_below_then_char",
            99,
            3000.0,
            vec![
                ("rr", b(|cm| rr(cm, "\nX", p(98, 7), None))),
                ("char", b(|cm| rr(cm, "X", p(80, 0), None))),
            ],
        ),
        numbered(
            "f3_below_then_scroll",
            99,
            3000.0,
            vec![
                ("rr", b(|cm| rr(cm, "\nX", p(98, 7), None))),
                scroll(3000.0 + 12.0 * 38.0),
            ],
        ),
    ]
}

struct Cmp {
    diffs: Vec<String>,
    compared: usize,
}
impl Cmp {
    #[allow(clippy::too_many_arguments)]
    fn check(
        &mut self,
        name: &str,
        label: &str,
        geometric: bool,
        got: &State,
        want: &State,
        glog: &[String],
        wlog: &[String],
    ) {
        self.compared += 1;
        let ok_state = if geometric {
            got.vp == want.vp
                && got.top == want.top
                && got.visible.0 >= got.vp.0
                && got.visible.1 <= got.vp.1
        } else {
            got == want
        };
        if !(ok_state && glog == wlog) {
            self.diffs.push(format!(
                "{name}/{label}:\n  rust {got:?}\n       {glog:?}\n  cm5  {want:?}\n       {wlog:?}"
            ));
        }
    }
    fn gutter(&mut self, name: &str, label: &str, changed: bool, step: &Value, before: &Value) {
        let cm5 = step["gutter"] != before["gutter"];
        if changed != cm5 {
            self.diffs.push(format!(
                "{name}/{label}: gutter width changed: rust {changed}, cm5 {cm5} ({} -> {})",
                before["gutter"], step["gutter"]
            ));
        }
    }
    fn finish(self) {
        println!("{} states compared", self.compared);
        assert!(self.diffs.is_empty(), "{}", self.diffs.join("\n"));
    }
}

fn run_probes(probes: Vec<Probe>) {
    let rec = rec();
    let mut cmp = Cmp {
        diffs: Vec::new(),
        compared: 0,
    };
    for pr in probes {
        let r = &rec["cases"][pr.name];
        assert!(r.is_object(), "{} is recorded", pr.name);
        let mut cm = editor_with(&pr.text, pr.options.clone());
        if let Some(s) = &pr.setup {
            s(&mut cm);
        }
        cm.scroll_to(None, Some(pr.top));
        let log: Log = Rc::default();
        watch(&cm, &log, "");
        cmp.check(
            pr.name,
            "before",
            pr.geometric,
            &state(&cm),
            &rstate(&r["before"]),
            &[],
            &[],
        );
        let mut prev = &r["before"];
        for (i, (label, act)) in pr.steps.iter().enumerate() {
            let g0 = cm.gutter_width;
            act(&mut cm);
            let got = std::mem::take(&mut *log.borrow_mut());
            let step = &r["steps"][i];
            cmp.check(
                pr.name,
                label,
                pr.geometric,
                &state(&cm),
                &rstate(&step["state"]),
                &got,
                &rlog(&step["log"]),
            );
            cmp.gutter(pr.name, label, cm.gutter_width != g0, &step["state"], prev);
            prev = &step["state"];
        }
    }
    cmp.finish();
}

/// Not one of F1–F4: CM5's `Doc.setValue` also calls `scrollToCoords(cm, 0, 0)`
/// (src/model/Doc.js:78). This pinned a known difference (the scroll position was
/// kept); `set_value` now scrolls to the top like CM5 (CODEMIRROR_R_SET_VALUE_SCROLL_01,
/// full matrix in set_value_scroll.rs).
#[test]
fn set_value_scrolls_to_top() {
    let rec = rec();
    let r = &rec["cases"]["n1_set_value_scrolled"]["steps"][1];
    let mut cm = editor_with(&lines(200), EditorOptions::default());
    cm.scroll_to(None, Some(UP));
    cm.set_value(&lines(200));
    assert_eq!(r["state"]["top"], 0.0, "CM5 scrolls to the top");
    assert_eq!(cm.display.scroll_top, 0.0, "Rust scrolls to the top");
}

#[test]
fn qa_probes_match_cm5() {
    run_probes(qa_probes());
}

#[test]
fn f1_line_changes_match_cm5() {
    run_probes(f1_probes());
}

#[test]
fn f3_gutter_width_changes_match_cm5() {
    run_probes(f3_probes());
}

/// p09: two direct document edits, each delivered by the host before the next.
#[test]
fn qa_p09_direct_doc_edits() {
    let rec = rec();
    let r = &rec["cases"]["p09_direct_doc_two_changes"];
    let mut cmp = Cmp {
        diffs: Vec::new(),
        compared: 0,
    };
    let mut cm = editor_with(&lines(200), EditorOptions::default());
    cm.scroll_to(None, Some(3000.0));
    let log: Log = Rc::default();
    watch(&cm, &log, "");
    for (i, (t, at)) in [("X\n", p(0, 0)), ("Y", p(100, 0))].into_iter().enumerate() {
        cm.doc.replace_range(t, at, None, None);
        assert!(log.borrow().is_empty(), "nothing before delivery (D6)");
        cm.flush_parked();
        let got = std::mem::take(&mut *log.borrow_mut());
        cmp.check(
            "p09",
            &format!("doc{}", i + 1),
            false,
            &state(&cm),
            &rstate(&r["steps"][i]["state"]),
            &got,
            &rlog(&r["steps"][i]["log"]),
        );
    }
    cmp.finish();
}

/// p07c and p11: native scrolls through the app.
#[test]
fn qa_p07c_p11_native_scroll() {
    let rec = rec();
    let mut cmp = Cmp {
        diffs: Vec::new(),
        compared: 0,
    };
    let mk = || {
        cm_core::clock::manual(1_000_000.0);
        let mut app = App::new(
            &lines(200),
            None,
            Metrics::oracle(),
            &AppOptions {
                no_line_numbers: true,
                mode: Some("text/plain".into()),
                ..Default::default()
            },
            Box::new(MemClipboard::default()),
        );
        app.handle(Input::Resize {
            width: 600.0,
            height: 300.0,
        });
        app.editor.scroll_to(None, Some(3000.0));
        app
    };
    let mut app = mk();
    let log: Log = Rc::default();
    watch(&app.editor, &log, "");
    let r = &rec["cases"]["p07c_native_scroll_1px"];
    cmp.check(
        "p07c",
        "before",
        false,
        &state(&app.editor),
        &rstate(&r["before"]),
        &[],
        &[],
    );
    app.handle(Input::Wheel { dx: 0.0, dy: 1.0 });
    let got = std::mem::take(&mut *log.borrow_mut());
    cmp.check(
        "p07c",
        "native",
        false,
        &state(&app.editor),
        &rstate(&r["steps"][0]["state"]),
        &got,
        &rlog(&r["steps"][0]["log"]),
    );

    let mut app = mk();
    let log: Log = Rc::default();
    watch(&app.editor, &log, "");
    let r = &rec["cases"]["p11_rapid_alternating"];
    cmp.check(
        "p11",
        "before",
        false,
        &state(&app.editor),
        &rstate(&r["before"]),
        &[],
        &[],
    );
    let labels = [
        "scroll3500",
        "insertAbove",
        "scroll2500",
        "deleteAbove",
        "scroll3000",
        "charInside",
        "native3300",
        "insertInside",
    ];
    for (i, label) in labels.iter().enumerate() {
        let e = &mut app.editor;
        match *label {
            "scroll3500" => e.scroll_to(None, Some(3500.0)),
            "insertAbove" => rr(e, "X\n", p(0, 0), None),
            "scroll2500" => e.scroll_to(None, Some(2500.0)),
            "deleteAbove" => rr(e, "", p(0, 0), Some(p(1, 0))),
            "scroll3000" => e.scroll_to(None, Some(3000.0)),
            "charInside" => rr(e, "X", p(80, 0), None),
            "native3300" => {
                app.handle(Input::Wheel { dx: 0.0, dy: 300.0 });
            }
            "insertInside" => rr(e, "Q\n", p(90, 0), None),
            _ => unreachable!(),
        }
        let got = std::mem::take(&mut *log.borrow_mut());
        cmp.check(
            "p11",
            label,
            false,
            &state(&app.editor),
            &rstate(&r["steps"][i]["state"]),
            &got,
            &rlog(&r["steps"][i]["log"]),
        );
    }
    cmp.finish();
}

/// Two linked editors at different scroll positions in one `EditorSet`.
fn linked(name: &str, a_top: &[f64], b_top: &[f64], steps: &[&str]) {
    let rec = rec();
    let r = &rec["cases"][name];
    let mut cmp = Cmp {
        diffs: Vec::new(),
        compared: 0,
    };
    let mut set = EditorSet::new();
    let a = set.insert(editor_with(&lines(200), EditorOptions::default()));
    let doc_b = set
        .get(a)
        .unwrap()
        .doc
        .linked_doc(LinkedDocOptions {
            shared_hist: true,
            from: None,
            to: None,
            mode: None,
        })
        .unwrap();
    let mut eb = Editor::with_doc(
        doc_b,
        Spec::Null,
        EditorOptions::default(),
        Metrics::oracle(),
        Rc::new(Registry::with_all_modes()),
    );
    eb.set_size(600.0, 300.0);
    let bid = set.insert(eb);
    for &t in a_top {
        set.run(a, |cx| cx.scroll_to(None, Some(t)));
    }
    for &t in b_top {
        set.run(bid, |cx| cx.scroll_to(None, Some(t)));
    }
    let log: Log = Rc::default();
    watch(set.get(a).unwrap(), &log, "A:");
    watch(set.get(bid).unwrap(), &log, "B:");
    let st = |set: &EditorSet, id| state(set.get(id).unwrap());
    cmp.check(
        name,
        "before.a",
        false,
        &st(&set, a),
        &rstate(&r["before"]["a"]),
        &[],
        &[],
    );
    cmp.check(
        name,
        "before.b",
        false,
        &st(&set, bid),
        &rstate(&r["before"]["b"]),
        &[],
        &[],
    );
    for (i, label) in steps.iter().enumerate() {
        let (b_from, b_to) = {
            let e = set.get(bid).unwrap();
            (vf(e), vt(e))
        };
        match *label {
            "A_insert_50" => set.run(a, |cx| cx.doc.replace_range("X\n", p(50, 0), None, None)),
            "B_insert_30" => set.run(bid, |cx| cx.doc.replace_range("Y\n", p(30, 0), None, None)),
            "A_delete_10" => set.run(a, |cx| {
                cx.doc.replace_range("", p(10, 0), Some(p(11, 0)), None)
            }),
            "A_char_A_first" => set.run(a, |cx| {
                let f = vf(cx);
                cx.doc.replace_range("X", p(f, 0), None, None)
            }),
            "B_char_B_first" => set.run(bid, |cx| {
                cx.doc.replace_range("Y", p(b_from, 0), None, None)
            }),
            "A_char_B_last" => set.run(a, |cx| {
                cx.doc.replace_range("Z", p(b_to - 1, 0), None, None)
            }),
            _ => unreachable!(),
        };
        let got = std::mem::take(&mut *log.borrow_mut());
        let s = &r["steps"][i];
        let wlog = rlog(&s["log"]);
        cmp.check(
            name,
            &format!("{label}.a"),
            false,
            &st(&set, a),
            &rstate(&s["a"]),
            &got,
            &wlog,
        );
        cmp.check(
            name,
            &format!("{label}.b"),
            false,
            &st(&set, bid),
            &rstate(&s["b"]),
            &got,
            &wlog,
        );
    }
    cmp.finish();
}

#[test]
fn qa_p08_linked_different_viewports() {
    linked(
        "p08_linked_different_viewports",
        &[3000.0],
        &[1000.0],
        &["A_insert_50", "B_insert_30", "A_delete_10"],
    );
}

#[test]
fn f1_linked_editors_with_distinct_retained_ranges() {
    linked(
        "f1_linked_retained",
        &[UP],
        &[1000.0, 1000.0 + 4.0 * 38.0],
        &["A_char_A_first", "B_char_B_first", "A_char_B_last"],
    );
}
