//! `cursorCoords`, `charCoords`, `coordsChar`, `lineAtHeight` and `heightAtLine`
//! (src/edit/methods.js:184-216) in every coordinate system, with
//! `intoCoordSystem` / `fromCoordSystem` (src/measurement/position_measurement.js:308,330),
//! against the CM5 5.65.21 recording `tests/data/cm5-coord-systems.json` (made by
//! `oracle/cm5-coord-systems.mjs` in Chromium).
//!
//! The recording page measures like [`Metrics::oracle`] and sizes the editor
//! 600x300. The editor's window position and the page scroll come from the
//! recording's `env` and are handed to the editor as the host would
//! (`Display::window_origin`, `Display::page_scroll`).

use cm_core::Pos;
use cm_core::pos::Sticky;
use cm_modes::Spec;
use cm_view::{CoordSys, CoordsStart, Editor, EditorOptions, Metrics};
use serde_json::Value;

const RECORDING: &str = include_str!("data/cm5-coord-systems.json");

fn lines(n: usize) -> String {
    (0..n)
        .map(|i| {
            if i == 85 {
                "h".repeat(120)
            } else {
                format!("line {i}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn mode(v: &Value) -> Option<CoordSys> {
    match v.as_str().unwrap() {
        "default" => None,
        "page" => Some(CoordSys::Page),
        "window" => Some(CoordSys::Window),
        "local" => Some(CoordSys::Local),
        "div" => Some(CoordSys::Div),
        "line" => Some(CoordSys::Line),
        m => panic!("mode {m}"),
    }
}

fn pos(v: &Value) -> Pos {
    Pos::new(v["line"].as_i64().unwrap(), v["ch"].as_i64().unwrap())
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

fn editor(name: &str, env: &Value) -> Editor {
    let mut cm = Editor::new(
        &lines(200),
        Spec::Null,
        EditorOptions {
            line_numbers: name == "line_numbers" || name == "first_line_5",
            first_line_number: if name == "first_line_5" { 5 } else { 1 },
            ..Default::default()
        },
        Metrics::oracle(),
    );
    cm.set_size(600.0, 300.0);
    cm.doc.set_selection(
        Pos::new(85, 3),
        Some(Pos::new(82, 1)),
        Some(cm_core::SelOptions {
            scroll: Some(false),
            ..Default::default()
        }),
    );
    cm.scroll_to(Some(f(&env["scroll"][0])), Some(f(&env["scroll"][1])));
    cm.display.window_origin = (f(&env["wrapper"][0]), f(&env["wrapper"][1]));
    cm.display.page_scroll = (f(&env["pageScroll"][0]), f(&env["pageScroll"][1]));
    cm
}

fn rect_eq(got: cm_view::measure::Rect, want: &Value) -> bool {
    got.left == f(&want["left"])
        && got.right == f(&want["right"])
        && got.top == f(&want["top"])
        && got.bottom == f(&want["bottom"])
}

#[test]
fn coord_systems_match_cm5() {
    let v: Value = serde_json::from_str(RECORDING).unwrap();
    assert_eq!(v["version"], "5.65.21");
    let mut failures = Vec::new();
    let mut checked = 0;
    for (name, c) in v["cases"].as_object().unwrap() {
        let env = &c["env"];
        let cm = editor(name, env);
        // The environment the comparisons rest on.
        assert_eq!(
            (cm.display.scroll_left, cm.display.scroll_top),
            (f(&env["scroll"][0]), f(&env["scroll"][1])),
            "{name}: scroll"
        );
        // `viewFrom` sets `viewOffset`. (`viewTo` can differ: CM5 builds the
        // editor 300 px high from its CSS, this one 600 px before `set_size`, and
        // updateDisplayIfNeeded keeps up to 20 lines already drawn.)
        assert_eq!(
            cm.display.view_from,
            env["viewport"][0].as_i64().unwrap(),
            "{name}: viewFrom"
        );
        assert_eq!(
            cm.view_offset(),
            f(&env["viewOffset"]),
            "{name}: viewOffset"
        );
        // The gutter width is the gutter code's (maybeUpdateLineNumberWidth); this
        // test takes CM5's so that only the coordinate code is compared. With line
        // numbers the port's is 1 px narrower (no `+ 1` on lineNumInnerWidth,
        // line_numbers.js:40), which the gutter work tracks.
        assert!(
            (cm.gutter_width - f(&env["gutters"])).abs() <= 1.0,
            "{name}: gutters {} vs {}",
            cm.gutter_width,
            env["gutters"]
        );
        let mut cm = cm;
        cm.gutter_width = f(&env["gutters"]);
        assert_eq!(cm.doc_height(), f(&env["docHeight"]), "{name}: doc height");

        for e in c["cursorCoords"].as_array().unwrap() {
            let start = match &e["start"] {
                Value::Null => CoordsStart::Head,
                Value::Bool(b) => CoordsStart::from(*b),
                p => CoordsStart::Pos(pos(p)),
            };
            let got = cm.cursor_coords_in(start, mode(&e["mode"]));
            checked += 1;
            if !rect_eq(got, &e["r"]) {
                failures.push(format!("{name} cursorCoords {e}: got {got:?}"));
            }
        }
        for e in c["charCoords"].as_array().unwrap() {
            let got = cm.char_coords_in(pos(&e["pos"]), mode(&e["mode"]));
            checked += 1;
            if !rect_eq(got, &e["r"]) {
                failures.push(format!("{name} charCoords {e}: got {got:?}"));
            }
        }
        for e in c["coordsChar"].as_array().unwrap() {
            let got = cm.coords_char_in(f(&e["left"]), f(&e["top"]), mode(&e["mode"]));
            let w = &e["p"];
            let sticky = match w["sticky"].as_str() {
                None => None,
                Some("before") => Some(Sticky::Before),
                Some("after") => Some(Sticky::After),
                Some(s) => panic!("sticky {s}"),
            };
            checked += 1;
            if got.line != w["line"].as_i64().unwrap()
                || got.ch != w["ch"].as_i64().unwrap()
                || got.sticky != sticky
                || got.outside as i64 != w["outside"].as_i64().unwrap()
                || got.x_rel != f(&w["xRel"])
            {
                failures.push(format!("{name} coordsChar {e}: got {got:?}"));
            }
        }
        for e in c["lineAtHeight"].as_array().unwrap() {
            let got = cm.line_at_height_in(f(&e["h"]), mode(&e["mode"]));
            checked += 1;
            if got != e["line"].as_i64().unwrap() {
                failures.push(format!("{name} lineAtHeight {e}: got {got}"));
            }
        }
        for e in c["heightAtLine"].as_array().unwrap() {
            let iw = e["includeWidgets"].as_bool().unwrap();
            let got = if let Some(n) = e["handle"].as_i64() {
                let h = cm.doc.get_line_handle(n).unwrap();
                cm.height_at_line_in(&h, mode(&e["mode"]), iw)
            } else {
                cm.height_at_line_in(e["line"].as_i64().unwrap(), mode(&e["mode"]), iw)
            };
            checked += 1;
            if got != f(&e["h"]) {
                failures.push(format!("{name} heightAtLine {e}: got {got}"));
            }
        }
    }
    assert!(checked > 900, "only {checked} comparisons");
    assert!(
        failures.is_empty(),
        "{} of {checked} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// `fromCoordSystem` inverts `intoCoordSystem` for every system that has an
/// inverse, and a deleted line's handle gives NaN.
#[test]
fn coord_system_round_trip_and_deleted_handle() {
    let mut cm = Editor::new(
        &lines(200),
        Spec::Null,
        EditorOptions::default(),
        Metrics::oracle(),
    );
    cm.set_size(600.0, 300.0);
    cm.scroll_to(Some(40.0), Some(3000.0));
    cm.display.window_origin = (17.0, 33.0);
    cm.display.page_scroll = (5.0, 250.0);
    let r = cm.char_coords(Pos::new(84, 2), CoordSys::Doc);
    for sys in [
        CoordSys::Page,
        CoordSys::Window,
        CoordSys::Local,
        CoordSys::Div,
        CoordSys::Doc,
    ] {
        let s = cm.char_coords_in(Pos::new(84, 2), Some(sys));
        let (x, y) = cm.from_coord_system(s.left, s.top, sys);
        assert_eq!((x, y + cm.view_offset()), (r.left, r.top), "{sys:?}");
        let p = cm.coords_char_in(s.left + 1.0, s.top + 1.0, Some(sys));
        assert_eq!((p.line, p.ch), (84, 2), "{sys:?}");
    }
    let h = cm.doc.get_line_handle(150).unwrap();
    cm.run_op(|cm| {
        cm.doc
            .replace_range("", Pos::new(149, 0), Some(Pos::new(151, 0)), None)
    });
    assert!(cm.height_at_line_in(&h, None, false).is_nan());
}
