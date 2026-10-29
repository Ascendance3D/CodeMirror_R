//! Cursor drawing (issue #51): `prepareSelection` / `drawSelectionCursor`
//! (src/display/selection.js:13-60) through `Editor::cursor_rects`, against
//! CodeMirror 5.65.21 recorded by `oracle/cm5-cursors.mjs`
//! (`tests/data/cm5-cursors.json`), and `toggleOverwrite`'s `overwriteToggle`
//! event (src/edit/methods.js:352).

use std::cell::RefCell;
use std::rc::Rc;

use cm_core::Pos;
use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, Metrics};
use serde_json::Value;

const DATA: &str = include_str!("data/cm5-cursors.json");

fn close(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() < 0.01, "{what}: port {a} vs CM5 {b}");
}

#[test]
fn cursors_match_cm5() {
    let data: Value = serde_json::from_str(DATA).unwrap();
    assert_eq!(data["version"], "5.65.21");
    let docs = |name: &str| -> String {
        match name {
            "short" => "alpha\nbeta gamma\n\tdelta\nend".to_string(),
            "long" => (0..60)
                .map(|i| format!("line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => panic!("doc {name}"),
        }
    };
    let mut n = 0;
    for (name, case) in data["cases"].as_object().unwrap() {
        let opts = &case["opts"];
        let o = EditorOptions {
            show_cursor_when_selecting: opts["showCursorWhenSelecting"].as_bool().unwrap_or(false),
            cursor_height: opts["cursorHeight"].as_f64().unwrap_or(1.0),
            fat_cursor: case["fat"].as_bool().unwrap(),
            ..Default::default()
        };
        let mut cm = Editor::new(
            &docs(case["doc"].as_str().unwrap()),
            Spec::Null,
            o,
            Metrics::oracle(),
        );
        cm.set_size(600.0, 300.0);
        let pos = |v: &Value| Pos::new(v[0].as_i64().unwrap(), v[1].as_i64().unwrap());
        let sel: Vec<(Pos, Option<Pos>)> = case["sel"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (pos(&r[0]), Some(pos(&r[1]))))
            .collect();
        cm.run_op(|cm| cm.doc.set_selections(&sel, Some(0), None));
        assert_eq!(
            cm.display.view_from,
            case["view"][0].as_i64().unwrap(),
            "{name} viewFrom"
        );
        let got = cm.cursor_rects();
        let want = case["cursors"].as_array().unwrap();
        assert_eq!(got.len(), want.len(), "{name}: {got:?}");
        for ((_, r), w) in got.iter().zip(want) {
            close(r.left, w["left"].as_f64().unwrap(), &format!("{name} left"));
            close(r.top, w["top"].as_f64().unwrap(), &format!("{name} top"));
            close(
                r.bottom - r.top,
                w["height"].as_f64().unwrap(),
                &format!("{name} height"),
            );
            match w["styleWidth"].as_f64() {
                Some(sw) => close(r.right - r.left, sw, &format!("{name} fat width")),
                None => assert_eq!(r.right, r.left, "{name}: a normal cursor is 0 wide"),
            }
            n += 1;
        }
    }
    assert!(n >= 15, "{n} cursors compared");
}

/// `toggleOverwrite(value)`: no change and no event when `value` equals the
/// state; otherwise the state flips and `overwriteToggle` fires (immediate).
#[test]
fn overwrite_toggle_event() {
    let mut cm = Editor::new(
        "abc",
        Spec::Null,
        EditorOptions::default(),
        Metrics::test_grid(),
    );
    let log = Rc::new(RefCell::new(Vec::<bool>::new()));
    let l = log.clone();
    cm.on_overwrite_toggle(move |cx, on| {
        assert_eq!(cx.overwrite, on);
        l.borrow_mut().push(on)
    });
    cm.toggle_overwrite(Some(false));
    assert!(log.borrow().is_empty());
    cm.toggle_overwrite(None);
    cm.toggle_overwrite(Some(true));
    cm.toggle_overwrite(Some(false));
    cm.exec_command("toggleOverwrite");
    assert_eq!(*log.borrow(), [true, false, true]);
    assert!(cm.overwrite);
}
