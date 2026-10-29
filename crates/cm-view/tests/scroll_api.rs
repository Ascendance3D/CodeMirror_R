//! `scrollTo`, `getScrollInfo` and `scrollIntoView` (src/edit/methods.js:364-390),
//! with `scrollToCoordsRange` and the rect helper `scrollIntoView(cm, rect)`
//! (src/display/scrolling.js:70,149), against the CM5 5.65.21 recording
//! `tests/data/cm5-scroll-api.json` (made by `oracle/cm5-scroll-api.mjs` in
//! Chromium, the page metrics of [`Metrics::oracle`], 600x300).

use cm_core::Pos;
use cm_modes::Spec;
use cm_view::measure::Rect;
use cm_view::{Editor, EditorOptions, Metrics, ScrollInfo, ScrollTarget};
use serde_json::Value;

const RECORDING: &str = include_str!("data/cm5-scroll-api.json");

fn doc(name: &str) -> String {
    match name {
        "long" => (0..60)
            .map(|i| {
                if i == 30 {
                    "y".repeat(150)
                } else {
                    format!("line {i}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        "short" => "a\nbb\nccc".into(),
        "wide" => vec!["x".repeat(150); 5].join("\n"),
        _ => unreachable!(),
    }
}

fn editor(text: &str, options: EditorOptions) -> Editor {
    let mut cm = Editor::new(text, Spec::Null, options, Metrics::oracle());
    cm.set_size(600.0, 300.0);
    cm
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

fn info(v: &Value) -> ScrollInfo {
    ScrollInfo {
        left: f(&v["left"]),
        top: f(&v["top"]),
        height: f(&v["height"]),
        width: f(&v["width"]),
        client_height: f(&v["clientHeight"]),
        client_width: f(&v["clientWidth"]),
    }
}

/// Equal, except that the horizontal extent may differ by under 1 px: the browser's
/// `scrollWidth` / `scrollLeft` are whole pixels, the port's longest-line width is
/// not (the known right-edge precision gap, as in scroll_clamp.rs).
fn same(g: &ScrollInfo, w: &ScrollInfo) -> bool {
    (g.top, g.height, g.client_height, g.client_width)
        == (w.top, w.height, w.client_height, w.client_width)
        && (g.left - w.left).abs() < 1.0
        && (g.width - w.width).abs() < 1.0
}

fn rect(left: f64, top: f64, right: f64, bottom: f64) -> Rect {
    Rect {
        left,
        right,
        top,
        bottom,
    }
}

fn recording() -> Value {
    let v: Value = serde_json::from_str(RECORDING).unwrap();
    assert_eq!(v["version"], "5.65.21");
    v
}

#[test]
fn get_scroll_info_matches_cm5() {
    let v = recording();
    let cases: [(&str, &str, EditorOptions, (f64, f64)); 8] = [
        ("long", "long", EditorOptions::default(), (0.0, 0.0)),
        (
            "long_scrolled",
            "long",
            EditorOptions::default(),
            (100.0, 500.0),
        ),
        ("short", "short", EditorOptions::default(), (0.0, 0.0)),
        ("wide", "wide", EditorOptions::default(), (0.0, 0.0)),
        ("wide_max", "wide", EditorOptions::default(), (1e9, 1e9)),
        (
            "long_numbers",
            "long",
            EditorOptions {
                line_numbers: true,
                ..Default::default()
            },
            (50.0, 700.0),
        ),
        (
            "wrapped",
            "wide",
            EditorOptions {
                line_wrapping: true,
                ..Default::default()
            },
            (0.0, 40.0),
        ),
        ("long_max", "long", EditorOptions::default(), (1e9, 1e9)),
    ];
    let mut failures = Vec::new();
    for (name, d, o, (x, y)) in cases {
        let mut cm = editor(&doc(d), o);
        cm.scroll_to(Some(x), Some(y));
        let got = cm.get_scroll_info();
        let want = info(&v["info"][name]);
        if !same(&got, &want) {
            failures.push(format!("{name}: got {got:?}, CM5 {want:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

type Act = fn(&mut Editor);

#[test]
fn scroll_into_view_forms_match_cm5() {
    let v = recording();
    let cases: Vec<(&str, f64, Act)> = vec![
        ("null", 0.0, |cm| {
            cm.scroll_into_view_target(ScrollTarget::Cursor, None)
        }),
        ("null_margin_option", 50.0, |cm| {
            cm.scroll_into_view_target(ScrollTarget::Cursor, None)
        }),
        ("null_margin_given", 50.0, |cm| {
            cm.scroll_into_view_target(ScrollTarget::Cursor, Some(20.0))
        }),
        ("null_margin_zero_given", 50.0, |cm| {
            cm.scroll_into_view_target(ScrollTarget::Cursor, Some(0.0))
        }),
        ("line_number", 0.0, |cm| {
            cm.scroll_into_view_target(50, None)
        }),
        ("line_number_margin", 0.0, |cm| {
            cm.scroll_into_view_target(50, Some(30.0))
        }),
        ("line_number_up", 0.0, |cm| {
            cm.scroll_into_view_target(5, None)
        }),
        ("pos", 0.0, |cm| {
            cm.scroll_into_view_target(Pos::new(30, 140), None)
        }),
        ("pos_margin", 0.0, |cm| {
            cm.scroll_into_view_target(Pos::new(20, 0), Some(40.0))
        }),
        ("range", 0.0, |cm| {
            cm.scroll_into_view_target(ScrollTarget::Range(Pos::new(30, 2), Pos::new(45, 0)), None)
        }),
        ("range_from_only", 0.0, |cm| {
            cm.scroll_into_view_target(Pos::new(50, 0), None)
        }),
        ("coords", 0.0, |cm| {
            cm.scroll_into_view_target(rect(100.0, 1200.0, 120.0, 1240.0), None)
        }),
        ("coords_margin", 0.0, |cm| {
            cm.scroll_into_view_target(rect(100.0, 1200.0, 120.0, 1240.0), Some(25.0))
        }),
        ("coords_wide", 0.0, |cm| {
            cm.scroll_into_view_target(rect(2000.0, 1200.0, 2100.0, 1240.0), None)
        }),
        ("coords_up", 0.0, |cm| {
            cm.scroll_into_view_target(rect(50.0, 300.0, 60.0, 330.0), None)
        }),
        ("coords_range", 0.0, |cm| {
            cm.scroll_into_view_target(
                ScrollTarget::CoordsRange(
                    rect(100.0, 1200.0, 120.0, 1240.0),
                    rect(500.0, 1400.0, 520.0, 1450.0),
                ),
                None,
            )
        }),
        ("coords_range_margin", 0.0, |cm| {
            cm.scroll_into_view_target(
                ScrollTarget::CoordsRange(
                    rect(100.0, 1200.0, 120.0, 1240.0),
                    rect(500.0, 1400.0, 520.0, 1450.0),
                ),
                Some(10.0),
            )
        }),
        ("coords_top_snap", 0.0, |cm| {
            cm.scroll_into_view_target(rect(0.0, 20.0, 0.0, 50.0), None)
        }),
        ("coords_bottom_snap", 0.0, |cm| {
            cm.scroll_into_view_target(rect(0.0, 2200.0, 0.0, 2290.0), None)
        }),
        ("coords_tall", 0.0, |cm| {
            cm.scroll_into_view_target(rect(0.0, 1000.0, 0.0, 1900.0), None)
        }),
        ("op_into_then_to", 0.0, |cm| {
            cm.run_op(|cm| {
                cm.scroll_into_view_target(Pos::new(50, 0), None);
                cm.scroll_to(None, Some(100.0));
            })
        }),
        ("op_to_then_into", 0.0, |cm| {
            cm.run_op(|cm| {
                cm.scroll_to(None, Some(100.0));
                cm.scroll_into_view_target(Pos::new(50, 0), None);
            })
        }),
        ("op_cursor_then_to", 0.0, |cm| {
            cm.run_op(|cm| {
                cm.doc.set_cursor(Pos::new(50, 0), None);
                cm.scroll_to(None, Some(100.0));
            })
        }),
        ("op_to_x_only", 0.0, |cm| {
            cm.run_op(|cm| {
                cm.scroll_into_view_target(Pos::new(50, 0), None);
                cm.scroll_to(Some(200.0), None);
            })
        }),
        ("op_to_null_null", 0.0, |cm| {
            cm.run_op(|cm| {
                cm.scroll_into_view_target(Pos::new(50, 0), None);
                cm.scroll_to(None, None);
            })
        }),
        ("op_coords_then_to", 0.0, |cm| {
            cm.run_op(|cm| {
                cm.scroll_into_view_target(rect(0.0, 1200.0, 0.0, 1240.0), None);
                cm.scroll_to(None, Some(100.0));
            })
        }),
    ];
    let mut failures = Vec::new();
    for (name, margin, act) in cases {
        let c = &v["cases"][name];
        assert!(c.is_object(), "{name} not recorded");
        let mut cm = editor(
            &doc("long"),
            EditorOptions {
                cursor_scroll_margin: margin,
                ..Default::default()
            },
        );
        cm.doc.set_cursor(
            Pos::new(40, 3),
            Some(cm_core::SelOptions {
                scroll: Some(false),
                ..Default::default()
            }),
        );
        cm.scroll_to(Some(f(&c["setup"][0])), Some(f(&c["setup"][1])));
        let before = cm.get_scroll_info();
        assert!(
            same(&before, &info(&c["before"])),
            "{name}: before {before:?}"
        );
        act(&mut cm);
        let got = cm.get_scroll_info();
        let want = info(&c["after"]);
        if !same(&got, &want) {
            failures.push(format!("{name}: got {got:?}, CM5 {want:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The rect helper `scrollIntoView(cm, rect)` (scrolling.js:70) scrolls at once,
/// through the thresholded `updateScrollTop` / `setScrollLeft`: a change under 2 px
/// is dropped, unlike the methodOp coordinate form, which sets `op.scrollTop`.
#[test]
fn rect_helper_is_immediate_and_thresholded() {
    let mut cm = editor(&doc("long"), EditorOptions::default());
    cm.scroll_to(None, Some(500.0));
    // The rect's top is 1 px above the view: calculateScrollPos asks for 499.
    cm.scroll_rect_into_view(rect(0.0, 499.0, 0.0, 520.0));
    assert_eq!(cm.display.scroll_top, 500.0);
    // 3 px above: applied.
    cm.scroll_rect_into_view(rect(0.0, 497.0, 0.0, 520.0));
    assert_eq!(cm.display.scroll_top, 497.0);
    // The methodOp form applies 1 px.
    cm.scroll_into_view_target(rect(0.0, 496.0, 0.0, 520.0), None);
    assert_eq!(cm.display.scroll_top, 496.0);
    // Far: the same as the coordinate form, and the rendered range follows.
    cm.scroll_rect_into_view(rect(100.0, 1200.0, 120.0, 1240.0));
    assert_eq!(cm.display.scroll_top, 940.0);
    let first = cm.line_at_height(940.0 - 4.0);
    assert!(cm.display.view_from <= first && first < cm.display.view_to);
}
