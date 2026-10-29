//! Line widgets against `tests/data/cm5-line-widgets.json` (recorded by
//! `oracle/cm5-line-widgets.mjs`; Chromium, Firefox and WebKit agree up to font
//! rounding). Pinned CM5 src/model/line_widget.js, src/measurement/widgets.js:4,
//! src/measurement/position_measurement.js:297-330 and :463.
//!
//! The recording sizes each node so that the widget box (the node plus
//! `.CodeMirror-linewidget`'s 0.1 px padding) is a whole number of px, so CM5's box height
//! and `widget.height` (`offsetHeight`) are both the step's height, as natively.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use cm_core::{LineWidget, LineWidgetOptions, Pos};
use cm_modes::Spec;
use cm_view::{CoordSys, Editor, EditorOptions, Metrics};
use serde_json::{Value, json};

fn opts(v: Option<&Value>) -> LineWidgetOptions {
    let v = v.cloned().unwrap_or(json!({}));
    LineWidgetOptions {
        above: v["above"] == true,
        insert_at: v["insertAt"].as_i64(),
        ..LineWidgetOptions::default()
    }
}

#[test]
fn line_widgets_match_cm5() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cm5-line-widgets.json");
    let d: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("cm5-line-widgets.json"))
            .unwrap();
    assert_eq!(d["version"], "5.65.21");
    let text: Vec<String> = (0..40).map(|i| format!("line {i} text")).collect();
    let mut steps = 0;
    for c in d["cases"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let o = EditorOptions {
            line_wrapping: c["opts"]["lineWrapping"] == true,
            ..EditorOptions::default()
        };
        let mut cm = Editor::new(&text.join("\n"), Spec::Null, o, Metrics::oracle());
        cm.set_size(600.0, 300.0);
        let updates = Rc::new(RefCell::new(0u32));
        let u = updates.clone();
        cm.on_update(move |_| *u.borrow_mut() += 1);
        let widgets: Rc<RefCell<Vec<LineWidget>>> = Rc::default();
        let log: Rc<RefCell<Vec<Value>>> = Rc::default();
        for (ev, f) in [
            ("lineWidgetAdded", 0),
            ("lineWidgetCleared", 1),
            ("lineWidgetChanged", 2),
        ] {
            let (l, ws) = (log.clone(), widgets.clone());
            let h = move |_: &mut cm_view::Cx, w: &LineWidget, line: i64| {
                let i = ws
                    .borrow()
                    .iter()
                    .position(|x| x == w)
                    .map_or(-1, |i| i as i64);
                l.borrow_mut().push(json!([ev, i, line]));
            };
            match f {
                0 => cm.on_line_widget_added(h),
                1 => cm.on_line_widget_cleared(h),
                _ => cm.on_line_widget_changed(h),
            };
        }
        let pad = cm.metrics.padding_top;
        for (i, r) in c["recs"].as_array().unwrap().iter().enumerate() {
            let st = r["step"].as_array().unwrap();
            let here = format!("{name} step {i} {}", r["step"]);
            log.borrow_mut().clear();
            let before = *updates.borrow();
            let num = |k: usize| st[k].as_f64().unwrap();
            match st[0].as_str().unwrap() {
                "add" => {
                    let w = cm.add_line_widget(
                        st[1].as_i64().unwrap(),
                        Rc::new(()),
                        num(2),
                        opts(st.get(3)),
                    );
                    widgets.borrow_mut().push(w);
                }
                "clear" => {
                    let w = widgets.borrow()[st[1].as_u64().unwrap() as usize].clone();
                    cm.remove_line_widget(&w);
                }
                "change" => {
                    let w = widgets.borrow()[st[1].as_u64().unwrap() as usize].clone();
                    cm.line_widget_changed(&w, num(2));
                }
                "scroll" => cm.scroll_to(None, Some(num(1))),
                "edit" => {
                    let a = st[1].as_i64().unwrap();
                    let to = st.get(3).and_then(Value::as_i64).map(|b| Pos::new(b, 0));
                    cm.doc.replace_range(
                        st[2].as_str().unwrap(),
                        Pos::new(a, 0),
                        to.or(Some(Pos::new(a, 0))),
                        None,
                    );
                }
                op => panic!("{op}"),
            }
            assert_eq!(
                Value::from(log.borrow().clone()),
                r["events"],
                "{here}: events"
            );
            log.borrow_mut().clear();
            cm.fire_orphan_delayed();
            assert_eq!(
                Value::from(log.borrow().clone()),
                r["late"],
                "{here}: late events"
            );
            assert_eq!(
                *updates.borrow() - before,
                r["updates"].as_u64().unwrap() as u32,
                "{here}: updates"
            );
            assert_eq!(
                cm.doc_height(),
                r["docHeight"].as_f64().unwrap(),
                "{here}: doc height"
            );
            assert_eq!(
                cm.display.scroll_top,
                r["scrollTop"].as_f64().unwrap(),
                "{here}: scrollTop"
            );
            let last = cm.doc.last_line();
            for t in r["tops"].as_array().unwrap() {
                let n = t[0].as_i64().unwrap();
                let (l, end) = if n > last { (last, true) } else { (n, false) };
                let base = cm.height_at_line(l)
                    + pad
                    + if end {
                        cm.doc_height() - cm.height_at_line(l)
                    } else {
                        0.0
                    };
                let without = base + if end { 0.0 } else { cm.widget_top_height(l) };
                assert_eq!(
                    (without, base),
                    (t[1].as_f64().unwrap(), t[2].as_f64().unwrap()),
                    "{here}: heightAtLine({n})"
                );
            }
            for (cc, ch) in r["cursor"]
                .as_array()
                .unwrap()
                .iter()
                .zip(r["coordsChar"].as_array().unwrap())
            {
                let n = cc[0].as_i64().unwrap();
                let p = cm.doc.clip_pos(Pos::new(n, 2));
                let c = cm.cursor_coords(p, CoordSys::Local);
                assert_eq!(
                    (c.left, c.top, c.bottom),
                    (
                        cc[1].as_f64().unwrap(),
                        cc[2].as_f64().unwrap(),
                        cc[3].as_f64().unwrap()
                    ),
                    "{here}: cursorCoords({n}, 2)"
                );
                let q = cm.coords_char_local(c.left + 1.0, c.top + 1.0);
                assert_eq!(
                    (q.line, q.ch),
                    (ch[1].as_i64().unwrap(), ch[2].as_i64().unwrap()),
                    "{here}: coordsChar near ({n}, 2)"
                );
            }
            for la in r["lineAtHeight"].as_array().unwrap() {
                let y = la[0].as_f64().unwrap();
                let got = cm.line_at_height(y - pad).min(last);
                assert_eq!(got, la[1].as_i64().unwrap(), "{here}: lineAtHeight({y})");
            }
            for wc in r["widgetCounts"].as_array().unwrap() {
                let n = wc[0].as_i64().unwrap();
                let k = cm
                    .doc
                    .line_info(n)
                    .and_then(|i| i.widgets)
                    .map_or(0, |w| w.len());
                assert_eq!(
                    k as u64,
                    wc[1].as_u64().unwrap(),
                    "{here}: lineInfo({n}).widgets"
                );
            }
            for (n, want) in r["order"].as_array().unwrap().iter().enumerate() {
                let ws = widgets.borrow();
                let got: Vec<i64> = cm
                    .doc
                    .get_line_handle(n as i64)
                    .map(|h| h.widgets())
                    .unwrap_or_default()
                    .iter()
                    .map(|w| ws.iter().position(|x| x == w).map_or(-1, |i| i as i64))
                    .collect();
                assert_eq!(Value::from(got), *want, "{here}: widgets of line {n}");
            }
            steps += 1;
        }
    }
    assert_eq!(steps, 26);
}

/// Widget boxes (`positionLineWidget`, src/display/update_line.js:177) at two
/// horizontal scroll positions, with line numbers and a 16 px gutter, and the
/// widget-hit rule of `eventInWidget` (src/measurement/widgets.js:20). The rects are
/// relative to the wrapper (x) and the document top (y).
#[test]
fn line_widget_boxes_match_cm5() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cm5-line-widgets.json");
    let d: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("cm5-line-widgets.json"))
            .unwrap();
    let text: Vec<String> = (0..12)
        .map(|i| format!("line {i} {}", "x".repeat(if i == 4 { 60 } else { 5 })))
        .collect();
    for b in d["boxes"].as_array().unwrap() {
        let scroll_left = b["scrollLeft"].as_f64().unwrap();
        let mut o = EditorOptions {
            line_numbers: true,
            gutters: vec!["g16".into()],
            ..EditorOptions::default()
        };
        o.gutter_class_widths = vec![("g16".into(), 16.0)];
        let mut cm = Editor::new(&text.join("\n"), Spec::Null, o, Metrics::oracle());
        cm.set_size(600.0, 300.0);
        assert_eq!(cm.gutter_width, b["gutters"].as_f64().unwrap());
        let mut ws = Vec::new();
        for s in b["specs"].as_array().unwrap() {
            let v = &s[2];
            let wo = LineWidgetOptions {
                above: v["above"] == true,
                cover_gutter: v["coverGutter"] == true,
                no_h_scroll: v["noHScroll"] == true,
                handle_mouse_events: v["handleMouseEvents"] == true,
                ..LineWidgetOptions::default()
            };
            ws.push(cm.add_line_widget(
                s[0].as_i64().unwrap(),
                Rc::new(()),
                s[1].as_f64().unwrap(),
                wo,
            ));
        }
        cm.scroll_to(Some(scroll_left), Some(0.0));
        let boxes = cm.line_widget_boxes();
        for (i, (w, r)) in ws.iter().zip(b["rects"].as_array().unwrap()).enumerate() {
            let (_, got) = boxes.iter().find(|(x, _)| x == w).expect("widget in view");
            let f = |k: usize| r[k].as_f64().unwrap();
            let ctx = format!("scrollLeft {scroll_left} widget {i}");
            assert_eq!(got.left, f(0), "{ctx}: left");
            assert_eq!((got.top, got.bottom), (f(2), f(3)), "{ctx}: top/bottom");
            if w.options().no_h_scroll {
                // `width: cm.display.wrapper.clientWidth` (update_line.js:183).
                assert!((got.right - f(1)).abs() < 0.5, "{ctx}: right {}", got.right);
            }
            // A point inside the box: a widget that does not handle mouse events
            // swallows it; one that does lets the editor take it.
            let (x, y) = (got.left.max(0.0) + 5.0, (got.top + got.bottom) / 2.0);
            let div_x = x - cm.gutter_width + cm.display.scroll_left;
            assert_eq!(
                cm.event_in_widget(div_x, y),
                !w.options().handle_mouse_events,
                "{ctx}: eventInWidget"
            );
        }
    }
}
