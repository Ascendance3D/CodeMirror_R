//! Generic options: `setOption` / `getOption` (src/edit/methods.js:43-52) with
//! the per-option handlers and `optionChange`, `defineOption`
//! (src/edit/options.js:24), `defineInitHook` and `finishInit`
//! (src/edit/CodeMirror.js:80-90, 216), `autofocus`, `lineNumberFor`
//! (src/line/utils_line.js:83) and `phrase` (methods.js:437).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use cm_core::Pos;
use cm_modes::Spec;
use cm_view::options::{
    InitHook, LineNumberFormatter, OldValue, OptionError, OptionValue, default_option,
    define_init_hook, define_option,
};
use cm_view::{Editor, EditorOptions, Metrics};

fn editor_with(o: EditorOptions) -> Editor {
    Editor::new("a\n\tb\nc", Spec::Null, o, Metrics::test_grid())
}
fn editor() -> Editor {
    editor_with(EditorOptions::default())
}

/// test.js "setOption"-style round trips: getOption reads what setOption stored;
/// optionChange fires after the handler, only when the value changes.
#[test]
fn set_get_and_option_change() {
    let mut cm = editor();
    let log = Rc::new(RefCell::new(Vec::<String>::new()));
    let l = log.clone();
    cm.on_option_change(move |cx, name| {
        let v = cx.get_option(name).unwrap();
        l.borrow_mut().push(format!("{name}={v:?}"));
    });
    assert_eq!(cm.get_option("tabSize"), Some(OptionValue::Number(4.0)));
    cm.set_option("tabSize", 8i64).unwrap();
    assert_eq!(cm.options.tab_size, 8);
    // The handler ran: the layout uses the new tab size.
    assert_eq!(
        cm.char_coords(Pos::new(1, 1), cm_view::CoordSys::Line).left,
        84.0
    );
    cm.set_option("tabSize", 8i64).unwrap();
    cm.set_option("lineWrapping", true).unwrap();
    assert!(cm.options.line_wrapping);
    cm.set_option("readOnly", true).unwrap();
    assert!(cm.is_read_only());
    assert_eq!(cm.get_option("nope"), None);
    assert_eq!(
        cm.set_option("nope", 1i64),
        Err(OptionError::Unknown("nope".into()))
    );
    assert_eq!(
        cm.set_option("tabSize", "x"),
        Err(OptionError::Type("tabSize".into()))
    );
    assert_eq!(
        *log.borrow(),
        [
            "tabSize=Number(8.0)",
            "lineWrapping=Bool(true)",
            "readOnly=Bool(true)"
        ]
    );
    // `mode` runs even when unchanged.
    cm.set_option("mode", OptionValue::Null).unwrap();
    assert_eq!(log.borrow().last().unwrap(), "mode=Null");
}

/// `lineNumbers`, `firstLineNumber` and `lineNumberFormatter` update the gutter
/// (updateGutters); `lineNumberFor` applies the formatter to `i + firstLineNumber`.
#[test]
fn line_number_for_and_gutter_options() {
    let mut cm = editor();
    assert_eq!(cm.gutter_width, 0.0);
    cm.set_option("lineNumbers", true).unwrap();
    assert!(cm.gutter_width > 0.0);
    assert_eq!(cm.line_number_for(0), "1");
    cm.set_option("firstLineNumber", 0i64).unwrap();
    assert_eq!(cm.line_number_for(2), "2");
    let f = LineNumberFormatter::new(|n| format!("<{n}>"));
    cm.set_option("lineNumberFormatter", OptionValue::Formatter(f.clone()))
        .unwrap();
    assert_eq!(cm.line_number_for(2), "<2>");
    assert_eq!(
        cm.get_option("lineNumberFormatter"),
        Some(OptionValue::Formatter(f))
    );
}

/// `phrase(text)`: the `phrases` option's entry, or the text itself.
#[test]
fn phrases() {
    let mut cm = editor();
    assert_eq!(cm.phrase("Search:"), "Search:");
    let mut p = HashMap::new();
    p.insert("Search:".to_string(), "Suche:".to_string());
    cm.set_option("phrases", OptionValue::Phrases(p)).unwrap();
    assert_eq!(cm.phrase("Search:"), "Suche:");
    assert_eq!(cm.phrase("Replace:"), "Replace:");
}

/// `defineOption`: a custom option's default, its handler with `CodeMirror.Init`
/// on a new editor (unless `notOnInit`) and with the old value on `setOption`.
#[test]
fn define_option_custom() {
    let calls = Rc::new(RefCell::new(Vec::<String>::new()));
    let c = calls.clone();
    define_option(
        "testMatchBrackets",
        OptionValue::Bool(false),
        Some(Rc::new(
            move |_cm: &mut Editor, v: &OptionValue, old: &OldValue| {
                c.borrow_mut().push(format!("{v:?} {old:?}"))
            },
        )),
        false,
    )
    .unwrap();
    let c = calls.clone();
    define_option(
        "testLater",
        OptionValue::Number(1.0),
        Some(Rc::new(
            move |_cm: &mut Editor, v: &OptionValue, _old: &OldValue| {
                c.borrow_mut().push(format!("later {v:?}"))
            },
        )),
        true,
    )
    .unwrap();
    assert_eq!(
        define_option("tabSize", OptionValue::Null, None, false),
        Err(OptionError::BuiltIn("tabSize".into()))
    );
    assert_eq!(
        default_option("testMatchBrackets"),
        Some(OptionValue::Bool(false))
    );
    assert_eq!(default_option("tabSize"), Some(OptionValue::Number(4.0)));
    let mut cm = editor();
    assert_eq!(*calls.borrow(), ["Bool(false) Init"]);
    assert_eq!(
        cm.get_option("testMatchBrackets"),
        Some(OptionValue::Bool(false))
    );
    cm.set_option("testMatchBrackets", true).unwrap();
    cm.set_option("testLater", 2i64).unwrap();
    assert_eq!(
        *calls.borrow(),
        [
            "Bool(false) Init",
            "Bool(true) Value(Bool(false))",
            "later Number(2.0)"
        ]
    );
    assert_eq!(cm.get_option("testLater"), Some(OptionValue::Number(2.0)));
}

/// `finishInit` runs before the init hooks, at the end of the constructor.
#[test]
fn finish_init_and_init_hooks() {
    let order = Rc::new(RefCell::new(Vec::<String>::new()));
    let o = order.clone();
    define_init_hook(InitHook::new(move |cm| {
        o.borrow_mut()
            .push(format!("hook {}", cm.get_value().len()))
    }));
    let o = order.clone();
    let cm = editor_with(EditorOptions {
        finish_init: Some(InitHook::new(move |cm| {
            o.borrow_mut().push("finishInit".into());
            cm.set_value("xyz");
        })),
        ..Default::default()
    });
    assert_eq!(*order.borrow(), ["finishInit", "hook 3"]);
    assert_eq!(cm.get_value(), "xyz");
}

thread_local! {
    static NOW: Cell<f64> = const { Cell::new(0.0) };
}

/// `autofocus`: the constructor focuses the input; `onFocus` runs from a 20 ms
/// timeout (the focus handler is registered after `input.focus()`).
#[test]
fn autofocus_focuses_after_20ms() {
    cm_core::clock::set_override(Some(Box::new(|| NOW.with(|n| n.get()))));
    NOW.with(|n| n.set(1000.0));
    let mut cm = editor_with(EditorOptions {
        autofocus: true,
        ..Default::default()
    });
    assert!(!cm.has_focus());
    assert_eq!(cm.next_timer_in(), Some(20.0));
    NOW.with(|n| n.set(1020.0));
    cm.run_timers();
    assert!(cm.has_focus());
    let mut plain = editor();
    plain.run_timers();
    assert!(!plain.has_focus());
    assert_eq!(plain.next_timer_in(), None);
    cm_core::clock::set_override(None);
}
