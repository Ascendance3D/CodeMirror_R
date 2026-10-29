//! String-named and custom events (`on`, `off`, `getHandlers`, `signal`,
//! src/util/event.js:11-47) on any emitter, the document, the editor, line
//! handles and markers, and `signalDOMEvent` (event.js:51), the hook that lets a
//! handler pre-empt the editor's own handling of an input event.

use std::cell::RefCell;
use std::rc::Rc;

use cm_core::{Args, Doc, Emitter, NamedHandler, Pos};
use cm_modes::Spec;
use cm_view::{DomEvent, Editor, EditorOptions, KeyEvent, Metrics, MouseEvent, NamedEditorHandler};

type Log = Rc<RefCell<Vec<String>>>;

fn editor(text: &str) -> Editor {
    Editor::new(
        text,
        Spec::Null,
        EditorOptions::default(),
        Metrics::test_grid(),
    )
}

fn key(k: &str) -> KeyEvent {
    KeyEvent {
        key: k.into(),
        ..Default::default()
    }
}

fn arg_str(args: Args<'_>, i: usize) -> String {
    args.get(i)
        .and_then(|a| a.downcast_ref::<&str>().map(|s| s.to_string()))
        .or_else(|| {
            args.get(i)
                .and_then(|a| a.downcast_ref::<i64>().map(|n| n.to_string()))
        })
        .unwrap_or_else(|| "?".into())
}

/// `CodeMirror.on/off/signal` on an arbitrary object (an addon's hint data):
/// values reach the handlers in order; `off` by function identity removes the
/// first registration; `getHandlers` of an unknown name is empty.
#[test]
fn emitter_on_signal_off() {
    let em = Emitter::new();
    let log: Log = Rc::default();
    let l = log.clone();
    let pick: Rc<NamedHandler> = Rc::new(move |a: Args<'_>| {
        l.borrow_mut()
            .push(format!("pick {} {}", arg_str(a, 0), arg_str(a, 1)))
    });
    em.on_rc("pick", pick.clone());
    em.on_rc("pick", pick.clone());
    let l = log.clone();
    let id = em.on("select", move |a| {
        l.borrow_mut().push(format!("select {}", arg_str(a, 0)))
    });
    assert!(em.has_handler("pick") && !em.has_handler("close"));
    assert_eq!(em.get_handlers("close").len(), 0);
    em.signal("pick", &[&"word", &3i64]);
    em.signal("select", &[&"x"]);
    assert!(em.off_fn("pick", &pick));
    em.signal("pick", &[&"again", &4i64]);
    assert!(em.off(id));
    assert!(!em.off(id));
    em.signal("select", &[&"y"]);
    assert_eq!(
        *log.borrow(),
        ["pick word 3", "pick word 3", "select x", "pick again 4"]
    );
}

/// getHandlers is a snapshot: a handler removed by an earlier one during a signal
/// still runs, one added during it does not (event.js builds new arrays).
#[test]
fn editor_named_signal_snapshot() {
    let mut cm = editor("abc");
    let log: Log = Rc::default();
    let l = log.clone();
    let second: Rc<NamedEditorHandler> = Rc::new(move |_, _| l.borrow_mut().push("second".into()));
    let (l, s2) = (log.clone(), second.clone());
    cm.on_event("fold", move |cx, a| {
        l.borrow_mut().push(format!("first {}", arg_str(a, 0)));
        // true the first time, false once it is gone.
        let gone = cx.off_event_fn("fold", &s2);
        l.borrow_mut().push(format!("off {gone}"));
        let l2 = l.clone();
        cx.on_event("fold", move |_, _| l2.borrow_mut().push("added".into()));
        // The handler has the editor: it can edit.
        cx.run_op(|cm, _| cm.doc.replace_range("Z", Pos::new(0, 0), None, None));
    });
    cm.on_event_rc("fold", second);
    assert_eq!(cm.get_handlers("fold").len(), 2);
    cm.signal_event("fold", &[&7i64]);
    assert_eq!(*log.borrow(), ["first 7", "off true", "second"]);
    assert_eq!(cm.get_value(), "Zabc");
    log.borrow_mut().clear();
    // Now: first (removes nothing left, adds another), added.
    cm.signal_event("fold", &[&8i64]);
    assert_eq!(*log.borrow(), ["first 8", "off false", "added"]);
    assert!(cm.get_handlers("unfold").is_empty());
}

/// Typed and named handlers share `off` by id; a named `"change"` is its own
/// entry and is not called by the typed `change` event.
#[test]
fn editor_named_off_and_typed_names() {
    let mut cm = editor("abc");
    let log: Log = Rc::default();
    let l = log.clone();
    let id = cm.on_event("change", move |_, _| {
        l.borrow_mut().push("named change".into())
    });
    let l = log.clone();
    cm.on_change(move |_, _| l.borrow_mut().push("typed change".into()));
    cm.run_op(|cm| cm.doc.replace_range("x", Pos::new(0, 0), None, None));
    assert_eq!(*log.borrow(), ["typed change"]);
    cm.signal_event("change", &[]);
    assert_eq!(*log.borrow(), ["typed change", "named change"]);
    assert!(cm.off(id));
    assert!(!cm.has_handler("change"));
}

/// The document, line handles and markers have named tables too.
#[test]
fn doc_line_marker_named_events() {
    let doc = Doc::new("one\ntwo");
    let log: Log = Rc::default();
    let l = log.clone();
    let id = doc.on_event("custom", move |d: &Doc, a| {
        l.borrow_mut()
            .push(format!("doc {} {}", d.line_count(), arg_str(a, 0)))
    });
    doc.signal_event("custom", &[&"v"]);
    assert_eq!(doc.get_handlers("custom").len(), 1);
    assert!(doc.off(id));
    doc.signal_event("custom", &[&"w"]);

    let line = doc.get_line_handle(1).unwrap();
    let l = log.clone();
    let lid = line.on_event("redraw", move |a| {
        l.borrow_mut().push(format!("line {}", arg_str(a, 0)))
    });
    line.signal_event("redraw", &[&1i64]);
    assert!(line.off(lid));
    line.signal_event("redraw", &[&2i64]);

    let m = doc
        .mark_text(Pos::new(0, 0), Pos::new(0, 2), Default::default())
        .unwrap()
        .text_marker();
    let l = log.clone();
    let mid = m.on_event("beforeFold", move |_| l.borrow_mut().push("marker".into()));
    m.signal_event("beforeFold", &[]);
    assert_eq!(m.get_handlers("beforeFold").len(), 1);
    assert!(m.off(mid));
    m.signal_event("beforeFold", &[]);
    assert_eq!(*log.borrow(), ["doc 2 v", "line 1", "marker"]);
}

/// `signalDOMEvent`: a `keydown` handler that calls `preventDefault` stops the key
/// binding (and `display.shift` is not set, as the JS returns before it); one
/// that sets `codemirrorIgnore` does the same; one that does neither leaves the
/// key to the editor.
#[test]
fn keydown_preempted_by_handler() {
    let mut cm = editor("abc");
    cm.doc.set_cursor(Pos::new(0, 3), None);
    let mode: Rc<RefCell<&str>> = Rc::new(RefCell::new("prevent"));
    let seen: Log = Rc::default();
    let (m, s) = (mode.clone(), seen.clone());
    cm.on_dom_event("keydown", move |_, e| {
        s.borrow_mut()
            .push(format!("{} {}", e.kind, e.key.as_ref().unwrap().key));
        match *m.borrow() {
            "prevent" => e.prevent_default(),
            "ignore" => e.set_codemirror_ignore(true),
            _ => {}
        }
    });
    let shift = KeyEvent {
        key: "Enter".into(),
        shift: true,
        ..Default::default()
    };
    assert!(!cm.key_down(&shift));
    assert!(!cm.display.shift);
    assert_eq!(cm.get_value(), "abc");
    *mode.borrow_mut() = "ignore";
    assert!(!cm.key_down(&key("Enter")));
    assert_eq!(cm.get_value(), "abc");
    *mode.borrow_mut() = "pass";
    assert!(cm.key_down(&key("Enter")));
    assert_eq!(cm.get_value(), "abc\n");
    assert_eq!(*seen.borrow(), ["keydown Enter"; 3]);
}

/// `keypress`, `mousedown` and `paste` are pre-empted the same way.
#[test]
fn keypress_mousedown_paste_preempted() {
    let mut cm = editor("abc\ndef");
    cm.set_size(400.0, 200.0);
    let block = Rc::new(RefCell::new(true));
    for kind in ["keypress", "mousedown", "paste"] {
        let b = block.clone();
        cm.on_dom_event(kind, move |_, e| {
            if *b.borrow() {
                e.prevent_default()
            }
        });
    }
    cm.doc.set_cursor(Pos::new(0, 0), None);
    assert!(!cm.key_press("x", &key("X")));
    cm.paste("PASTE");
    let click = MouseEvent {
        x: 15.0,
        y: 25.0,
        button: 1,
        time: 1000.0,
        ..Default::default()
    };
    cm.mouse_down(&click);
    cm.mouse_up(Some(&click));
    assert_eq!(cm.get_value(), "abc\ndef");
    assert_eq!(cm.doc.get_cursor(None), Pos::new(0, 0));
    *block.borrow_mut() = false;
    assert!(cm.key_press("x", &key("X")));
    cm.paste("P");
    assert_eq!(cm.get_value(), "xPabc\ndef");
    cm.mouse_down(&click);
    cm.mouse_up(Some(&click));
    assert_ne!(cm.doc.get_cursor(None), Pos::new(0, 2));
}

/// `signalDOMEvent(cm, e, override)` signals the override name; with no handler
/// the event is not pre-empted.
#[test]
fn signal_dom_event_override_and_no_handler() {
    let mut cm = editor("abc");
    let e = DomEvent::new("mousedown");
    assert!(!cm.signal_dom_event(&e, None));
    cm.on_dom_event("contextmenu", |_, e| e.prevent_default());
    assert!(!cm.signal_dom_event(&e, None));
    assert!(cm.signal_dom_event(&e, Some("contextmenu")));
    assert!(e.default_prevented());
}

/// `maybeScrollWindow` returns early when a `scrollCursorIntoView` handler
/// prevents it (scrolling.js:16): the adapter gets no window rect. The editor's
/// own scroll still happens.
#[test]
fn scroll_cursor_into_view_veto() {
    let text = (0..100)
        .map(|i| format!("l{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut cm = editor(&text);
    cm.set_size(400.0, 200.0);
    cm.run_op(|cm| cm.doc.set_cursor(Pos::new(60, 0), None));
    assert!(cm.take_window_scroll_rect().is_some());
    let id = cm.on_dom_event("scrollCursorIntoView", |_, e| e.prevent_default());
    let before = cm.display.scroll_top;
    cm.run_op(|cm| cm.doc.set_cursor(Pos::new(5, 0), None));
    assert!(cm.take_window_scroll_rect().is_none());
    assert!(cm.display.scroll_top < before);
    cm.off(id);
    cm.run_op(|cm| cm.doc.set_cursor(Pos::new(90, 0), None));
    assert!(cm.take_window_scroll_rect().is_some());
}
