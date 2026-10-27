//! Test-only string adapter for the typed events (decision D7): `on(cm, "change",
//! f)` registers the typed handler for a JS event name and hands `f` the payload
//! formatted the way the CM5 recordings print it (`cm5-events.mjs`, `watch`). The
//! public API stays typed; this exists so tests can be written against the
//! recorded logs.

use cm_core::{Doc, ListenerId, Pos};
use cm_view::Editor;

fn ps(p: Pos) -> String {
    format!("{}:{}", p.line, p.ch)
}

fn opt(v: Option<i64>) -> String {
    v.map_or("null".into(), |v| v.to_string())
}

/// `cm.on(name, …)`: `f` gets the formatted arguments after `cm` (empty when the
/// recording prints none). Panics on a name the port does not have.
pub fn on(cm: &Editor, name: &str, f: impl Fn(String) + 'static) -> ListenerId {
    match name {
        "beforeChange" => {
            cm.on_before_change(move |_, _, c| f(format!("({},{:?})", ps(c.from), c.text)))
        }
        "beforeSelectionChange" => cm.on_before_selection_change(move |_, _, _| f(String::new())),
        "change" => cm.on_change(move |_, c| f(format!("({},{:?})", ps(c.from), c.text))),
        "changes" => cm.on_changes(move |_, cs| f(format!("({})", cs.len()))),
        "cursorActivity" => cm.on_cursor_activity(move |_| f(String::new())),
        "update" => cm.on_update(move |_| f(String::new())),
        "viewportChange" => cm.on_viewport_change(move |_, a, b| f(format!("({a},{b})"))),
        "markerAdded" => cm.on_marker_added(move |_, _| f(String::new())),
        "markerCleared" => {
            cm.on_marker_cleared(move |_, _, a, b| f(format!("({},{})", opt(a), opt(b))))
        }
        "keyHandled" => cm.on_key_handled(move |_, n| f(format!("({n})"))),
        "inputRead" => cm.on_input_read(move |_, c| f(format!("({},{:?})", ps(c.from), c.text))),
        "electricInput" => cm.on_electric_input(move |_, l| f(format!("({l})"))),
        "focus" => cm.on_focus(move |_| f(String::new())),
        "blur" => cm.on_blur(move |_| f(String::new())),
        _ => panic!("no editor event {name:?}"),
    }
}

/// `doc.on(name, …)`, formatted like the recording (`(from)` when the payload has a
/// position).
pub fn on_doc(doc: &Doc, name: &str, f: impl Fn(String) + 'static) -> ListenerId {
    match name {
        "beforeChange" => doc.on_before_change(move |_, c| f(format!("({})", ps(c.from)))),
        "beforeSelectionChange" => doc.on_before_selection_change(move |_, _| f(String::new())),
        "change" => doc.on_change(move |_, c| f(format!("({})", ps(c.from)))),
        "cursorActivity" => doc.on_cursor_activity(move |_| f(String::new())),
        "historyAdded" => doc.on_history_added(move |_| f(String::new())),
        _ => panic!("no document event {name:?}"),
    }
}

/// The recording's `watch(cm, tag, log, which)`: names starting with `doc.` go to
/// the editor's document. Each event appends `"{tag}:cm.{name}{args}"` (or
/// `"{tag}:doc.{name}{args}"`) to `log`.
pub fn watch(cm: &Editor, tag: &str, log: &super::Log, which: &[&str]) {
    for &name in which {
        let (l, line) = match name.strip_prefix("doc.") {
            Some(n) => (log.clone(), format!("{tag}:doc.{n}")),
            None => (log.clone(), format!("{tag}:cm.{name}")),
        };
        let f = move |args: String| l.borrow_mut().push(format!("{line}{args}"));
        match name.strip_prefix("doc.") {
            Some(n) => {
                on_doc(&cm.doc, n, f);
            }
            None => {
                on(cm, name, f);
            }
        }
    }
}
