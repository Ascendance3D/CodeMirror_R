//! The `gutters` option, gutter columns and `gutterClick`/`gutterContextMenu` against
//! `tests/data/cm5-gutters.json` (recorded by `oracle/cm5-gutters.mjs`; Chromium,
//! Firefox and WebKit give the same specs, columns, widths and events). Pinned CM5
//! src/display/gutters.js:6-44, src/display/update_display.js (updateGutterSpace) and
//! src/edit/mouse_events.js:370-417. The page CSS of the recording becomes
//! `gutter_class_widths`.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, GutterSpec, Metrics, MouseEvent};
use serde_json::{Value, json};

fn spec(v: &Value) -> GutterSpec {
    match v {
        Value::String(s) => GutterSpec::new(s),
        o => GutterSpec {
            class_name: o["className"].as_str().unwrap().into(),
            style: o["style"].as_str().map(Into::into),
        },
    }
}

#[test]
fn gutters_match_cm5() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cm5-gutters.json");
    let d: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("cm5-gutters.json")).unwrap();
    assert_eq!(d["version"], "5.65.21");
    let cases = d["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 12);
    let text: Vec<String> = (0..30).map(|i| format!("line {i}")).collect();
    let mut checked = 0;
    for c in cases {
        let name = c["name"].as_str().unwrap();
        let o = &c["opts"];
        let mut opts = EditorOptions {
            line_numbers: o["lineNumbers"].as_bool().unwrap_or(false),
            first_line_number: o["firstLineNumber"].as_i64().unwrap_or(1),
            gutters: o["gutters"]
                .as_array()
                .map(|a| a.iter().map(spec).collect())
                .unwrap_or_default(),
            ..EditorOptions::default()
        };
        opts.gutter_class_widths = vec![
            ("g16".into(), 16.0),
            ("g30".into(), 30.0),
            ("gw".into(), 7.0),
        ];
        let mut cm = Editor::new(&text.join("\n"), Spec::Null, opts, Metrics::oracle());
        cm.set_size(600.0, 300.0);
        match name {
            "set_option_gutters" => cm.set_gutters(vec!["g30".into()]),
            "set_option_line_numbers_off" => cm.set_line_numbers(false),
            _ => {}
        }
        let specs: Vec<Value> = cm
            .gutter_specs
            .iter()
            .map(|s| json!([s.class_name, s.style]))
            .collect();
        assert_eq!(Value::from(specs), c["specs"], "{name}: specs");
        let cols: Vec<(String, f64, f64)> = cm
            .gutter_columns
            .iter()
            .map(|g| {
                (
                    format!("CodeMirror-gutter {}", g.class_name),
                    g.left,
                    g.width,
                )
            })
            .collect();
        let want: Vec<(String, f64, f64)> = c["columns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                (
                    v[0].as_str().unwrap().into(),
                    v[1].as_f64().unwrap(),
                    v[2].as_f64().unwrap(),
                )
            })
            .collect();
        assert_eq!(cols, want, "{name}: columns");
        assert_eq!(
            cm.gutter_width,
            c["guttersWidth"].as_f64().unwrap(),
            "{name}: gutters width"
        );

        let log: Rc<RefCell<Vec<Value>>> = Rc::default();
        let l = log.clone();
        cm.on_gutter_click(move |_, line, cls, _| l.borrow_mut().push(json!(["click", line, cls])));
        if o["ctx"] == true {
            let l = log.clone();
            cm.on_gutter_context_menu(move |_, line, cls, _| {
                l.borrow_mut().push(json!(["ctx", line, cls]))
            });
        }
        for ev in c["events"].as_array().unwrap() {
            let (x, y) = (ev["x"].as_f64().unwrap(), ev["y"].as_f64().unwrap());
            let scroll = ev["scroll"].as_f64().unwrap();
            if scroll > 0.0 {
                cm.scroll_to(None, Some(scroll));
                assert_eq!(
                    cm.display.scroll_top,
                    ev["scrollTop"].as_f64().unwrap(),
                    "{name}: scroll"
                );
            }
            log.borrow_mut().clear();
            let button = ev["button"].as_u64().unwrap() as u8;
            let e = MouseEvent {
                x,
                y,
                button,
                shift: false,
                ctrl: false,
                alt: false,
                meta: false,
                time: 0.0,
            };
            let swallowed = if button == 3 {
                cm.context_menu_in_gutter(&e, x, y)
            } else {
                cm.click_in_gutter(&e, x, y)
            };
            let here = format!("{name} at ({x}, {y}) scroll {scroll} button {button}");
            assert_eq!(
                Value::from(log.borrow().clone()),
                ev["log"],
                "{here}: events"
            );
            if button == 1 {
                // A click outside the gutters reaches the text and moves the cursor.
                assert_eq!(
                    !swallowed,
                    ev["cursorMoved"].as_bool().unwrap(),
                    "{here}: reached text"
                );
            } else if !ev["log"].as_array().unwrap().is_empty() {
                assert!(
                    swallowed && ev["defaultPrevented"] == true,
                    "{here}: prevented"
                );
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 115);
}

/// `getGutters` (gutters.js:6): a duplicate line-number entry stays, the
/// line-number column is appended only when no entry names it.
#[test]
fn get_gutters_rules() {
    let g = |v: &[&str]| v.iter().map(|s| GutterSpec::new(s)).collect::<Vec<_>>();
    let names = |v: Vec<GutterSpec>| v.into_iter().map(|s| s.class_name).collect::<Vec<_>>();
    assert_eq!(
        names(cm_view::get_gutters(&g(&["a"]), true)),
        ["a", "CodeMirror-linenumbers"]
    );
    assert_eq!(
        names(cm_view::get_gutters(
            &g(&["CodeMirror-linenumbers", "a"]),
            false
        )),
        ["a"]
    );
    assert_eq!(
        names(cm_view::get_gutters(
            &g(&["CodeMirror-linenumbers", "CodeMirror-linenumbers"]),
            true
        )),
        ["CodeMirror-linenumbers", "CodeMirror-linenumbers"]
    );
    assert!(cm_view::get_gutters(&[], false).is_empty());
}
