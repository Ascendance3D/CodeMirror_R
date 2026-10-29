//! `cm-web`: the browser adapter of CodeMirror_R, for `wasm32-unknown-unknown`.
//!
//! It connects browser services to the shared engine (`cm-core`, `cm-modes`,
//! `cm-view` without its native front end). It holds no document, tokenizer,
//! history or key map of its own: every edit goes through `cm_view::app::App`
//! and `cm_view::Editor`.
//!
//! * [`WebEditor`] — create an editor (headless, or [`WebEditor::attach`]ed to an
//!   element), load and read text and selections, select a mode, feed input,
//!   render, refresh, destroy.
//! * [`install_clock`] / [`remove_clock`] — the host clock (`Date.now()`) behind
//!   `cm_core::clock::set_host_clock`. Attaching an editor installs `Date.now()`
//!   when no host clock is installed.
//! * [`ModeRegistry`] — a mode registry to build editors from, and `runMode`.
//! * `session` — the browser event rules (key, `beforeinput`, composition,
//!   clipboard), independent of the DOM. `frame` — what a frame shows. `dom` —
//!   elements, handlers, measurement and scheduling (`requestAnimationFrame`,
//!   the blink interval, the highlight worker timeout).
//!
//! Thread model: one module instance is one thread. Host callbacks (the clock,
//! file functions) must not call back into the editor that called them; such a
//! call returns an error instead of re-entering.

pub mod clock;
pub(crate) mod dom;
pub mod files;
pub mod frame;
pub mod session;

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use cm_core::{LineWhere, Pos};
use cm_modes::{Registry, Spec};
use cm_view::app::{App, AppOptions, Clipboard, Input};
use cm_view::{Editor, EditorOptions, Indent, KeyEvent, Metrics};
use js_sys::{Array, Function, Object, Reflect};
use wasm_bindgen::prelude::*;

use crate::dom::Dom;
use crate::session::Session;

thread_local! {
    static LIVE: Cell<u32> = const { Cell::new(0) };
}

/// The clipboard of an editor whose system clipboard is reached only through
/// browser events: it remembers the text of the last copy or cut. Reading the
/// system clipboard outside a `paste` event goes through
/// [`WebEditor::paste_from_system`] (permission and user-gesture rules apply).
#[derive(Default)]
pub struct EventClipboard(pub Option<String>);

impl Clipboard for EventClipboard {
    fn get(&mut self) -> Option<String> {
        self.0.clone()
    }
    fn set(&mut self, text: &str) {
        self.0 = Some(text.to_string());
    }
}

pub(crate) struct Inner {
    pub session: Session,
    pub dom: Option<Dom>,
}

impl Inner {
    fn new(app: App) -> Inner {
        LIVE.with(|l| l.set(l.get() + 1));
        Inner {
            session: Session::new(app),
            dom: None,
        }
    }

    fn editor(&mut self) -> &mut Editor {
        &mut self.session.app.editor
    }

    pub(crate) fn schedule_frame(&mut self) {
        if let Some(d) = self.dom.as_mut() {
            d.schedule_frame();
        }
    }

    pub(crate) fn render(&mut self) {
        self.update_metrics();
        let focused = self.session.app.editor.has_focus();
        let f = frame::build(&self.session.app.editor, focused);
        let tab = self.session.app.editor.options.tab_size;
        let pre = self.session.preedit().map(str::to_string);
        if let Some(d) = self.dom.as_mut() {
            let _ = d.apply(&f, tab, pre.as_deref());
        }
        // After the DOM is current: `maybeScrollWindow` measures it.
        if let Some(rect) = self.session.app.editor.take_window_scroll_rect() {
            if let Some(d) = self.dom.as_ref() {
                d.maybe_scroll_window(rect);
            }
        }
    }

    /// Measure the font when the cache lacks a value (a hidden element measured 0,
    /// or a window resize cleared it), and give a changed result to the editor
    /// through `set_metrics`, the `refresh()` port.
    fn update_metrics(&mut self) {
        let Some(m) = self.dom.as_mut().and_then(|d| d.measure_uncached()) else {
            return;
        };
        if m != self.session.app.editor.metrics {
            self.editor().set_metrics(m);
        }
    }

    /// After an input event: redraw, restart the blink, highlight later.
    pub(crate) fn after_input(&mut self, worker_ms: i32) {
        let focused = self.session.app.editor.has_focus();
        if let Some(d) = self.dom.as_mut() {
            d.schedule_frame();
            d.restart_blink(focused);
            d.start_worker(worker_ms);
        }
    }

    pub(crate) fn after_scroll(&mut self) {
        if let Some(d) = self.dom.as_mut() {
            d.schedule_frame();
            d.start_worker(100);
        }
    }

    pub(crate) fn restart_blink(&mut self) {
        let focused = self.session.app.editor.has_focus();
        if let Some(d) = self.dom.as_mut() {
            d.restart_blink(focused);
        }
    }

    /// One `highlightWorker` slice: up to `workTime` ms, then again after
    /// `workDelay` while lines remain.
    pub(crate) fn highlight_slice(&mut self) {
        let end = js_sys::Date::now() + dom::WORK_TIME;
        let mut more = true;
        while more && js_sys::Date::now() < end {
            self.session.app.deliver_pending();
            more = self.session.app.editor.highlight_work(20);
        }
        if let Some(d) = self.dom.as_mut() {
            d.schedule_frame();
            if more {
                d.start_worker(dom::WORK_DELAY);
            }
        }
    }

    pub(crate) fn resized(&mut self) {
        let Some((w, h)) = self.dom.as_ref().map(|d| d.client_size()) else {
            return;
        };
        let ed = &self.session.app.editor;
        if ed.display.width != w || ed.display.height != h {
            self.session.handle(Input::Resize {
                width: w,
                height: h,
            });
            self.render();
        }
    }

    fn detach(&mut self) {
        if let Some(mut d) = self.dom.take() {
            d.teardown();
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.detach();
        LIVE.with(|l| l.set(l.get() - 1));
    }
}

fn opt<T>(o: &JsValue, key: &str, f: impl FnOnce(JsValue) -> Option<T>) -> Option<T> {
    if !o.is_object() {
        return None;
    }
    Reflect::get(o, &JsValue::from_str(key))
        .ok()
        .filter(|v| !v.is_undefined() && !v.is_null())
        .and_then(f)
}

/// `mac` from src/util/browser.js: `ios || /Mac/.test(platform)`, where `ios` is
/// Safari (`/Apple Computer/.test(vendor)`) with `/Mobile\/\w+/` in the user agent
/// or more than two touch points (iPadOS reports a Mac platform).
fn is_mac() -> bool {
    let Some(nav) = web_sys::window().map(|w| w.navigator()) else {
        return false;
    };
    // web-sys has no `vendor` binding.
    let safari = Reflect::get(&nav, &JsValue::from_str("vendor"))
        .ok()
        .and_then(|v| v.as_string())
        .is_some_and(|v| v.contains("Apple Computer"));
    let ua = nav.user_agent().unwrap_or_default();
    let mobile_ua = ua.match_indices("Mobile/").any(|(i, m)| {
        ua[i + m.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    });
    let ios = safari && (mobile_ua || nav.max_touch_points() > 2);
    ios || nav.platform().is_ok_and(|p| p.contains("Mac"))
}

/// Options: `lineNumbers`, `lineWrapping`, `readOnly`, `tabSize`, `indentUnit`,
/// `indentWithTabs`, `smartIndent`, `lineWiseCopyCut`, `mac`, and `metrics`
/// (`"grid"` 10x20, `"oracle"`, or `{charWidth, textHeight}`) for a headless
/// editor; an attached editor measures its font.
fn editor_options(o: &JsValue) -> (EditorOptions, Metrics) {
    let mut e = EditorOptions {
        line_numbers: false,
        mac: is_mac(),
        ..Default::default()
    };
    let b = |v: JsValue| v.as_bool();
    let n = |v: JsValue| v.as_f64();
    if let Some(v) = opt(o, "lineNumbers", b) {
        e.line_numbers = v;
    }
    if let Some(v) = opt(o, "lineWrapping", b) {
        e.line_wrapping = v;
    }
    if let Some(v) = opt(o, "readOnly", b) {
        e.read_only = v;
    }
    if let Some(v) = opt(o, "tabSize", n) {
        e.tab_size = v as i64;
    }
    if let Some(v) = opt(o, "indentUnit", n) {
        e.indent_unit = v as i64;
    }
    if let Some(v) = opt(o, "indentWithTabs", b) {
        e.indent_with_tabs = v;
    }
    if let Some(v) = opt(o, "smartIndent", b) {
        e.smart_indent = v;
    }
    if let Some(v) = opt(o, "lineWiseCopyCut", b) {
        e.line_wise_copy_cut = v;
    }
    if let Some(v) = opt(o, "mac", b) {
        e.mac = v;
    }
    let metrics = match opt(o, "metrics", Some) {
        Some(v) if v.as_string().as_deref() == Some("oracle") => Metrics::oracle(),
        Some(v) if v.is_object() => Metrics::monospace(
            opt(&v, "charWidth", n).unwrap_or(10.0),
            opt(&v, "textHeight", n).unwrap_or(20.0),
        ),
        _ => Metrics::test_grid(),
    };
    (e, metrics)
}

fn app_options(e: &EditorOptions) -> AppOptions {
    AppOptions {
        read_only: e.read_only,
        line_wrapping: e.line_wrapping,
        no_line_numbers: !e.line_numbers,
        mode: None,
    }
}

fn busy() -> JsValue {
    js_sys::Error::new("cm-web: the editor is busy (re-entrant call from a host callback)").into()
}

fn destroyed() -> JsValue {
    js_sys::Error::new("cm-web: the editor was destroyed").into()
}

fn pos(line: f64, ch: f64) -> Pos {
    Pos::new(line as i64, ch as i64)
}

fn pos_array(p: Pos) -> Array {
    Array::of2(&JsValue::from(p.line as f64), &JsValue::from(p.ch as f64))
}

fn tokens_array(toks: impl IntoIterator<Item = (String, Option<String>)>) -> Array {
    toks.into_iter()
        .map(|(s, t)| {
            let ty = t.map_or(JsValue::NULL, |t| JsValue::from_str(&t));
            JsValue::from(Array::of2(&JsValue::from_str(&s), &ty))
        })
        .collect()
}

/// A mode registry: empty (`null` and `text/plain` only) or with every ported mode.
#[wasm_bindgen]
pub struct ModeRegistry {
    reg: Registry,
}

#[wasm_bindgen]
impl ModeRegistry {
    /// A registry with the `null` mode and `text/plain` only.
    #[wasm_bindgen(constructor)]
    pub fn new() -> ModeRegistry {
        ModeRegistry {
            reg: Registry::new(),
        }
    }

    /// A registry with every mode `cm-modes` ports.
    #[wasm_bindgen(js_name = withAllModes)]
    pub fn with_all_modes() -> ModeRegistry {
        ModeRegistry {
            reg: Registry::with_all_modes(),
        }
    }

    /// Register a ported mode by module name (`clike`, `css`, `htmlmixed`,
    /// `javascript`, `rust`, `toml`, `vrml`, `xml`). Returns false for others.
    pub fn register(&mut self, name: &str) -> bool {
        let r = &mut self.reg;
        match name {
            "clike" => cm_modes::clike::register(r),
            "css" => cm_modes::css::register(r),
            "htmlmixed" => cm_modes::htmlmixed::register(r),
            "javascript" => cm_modes::javascript::register(r),
            "rust" => cm_modes::rust::register(r),
            "toml" => cm_modes::toml::register(r),
            "vrml" => cm_modes::vrml::register(r),
            "xml" => cm_modes::xml::register(r),
            _ => return false,
        }
        true
    }

    #[wasm_bindgen(js_name = hasMode)]
    pub fn has_mode(&self, name: &str) -> bool {
        self.reg.has_mode(name)
    }

    #[wasm_bindgen(js_name = hasMime)]
    pub fn has_mime(&self, mime: &str) -> bool {
        self.reg.has_mime(mime)
    }

    /// `CodeMirror.runMode(text, spec)`: `[[text, style|null], ...]`, with `"\n"`
    /// between lines.
    #[wasm_bindgen(js_name = runMode)]
    pub fn run_mode(&self, text: &str, spec: &str) -> Array {
        tokens_array(cm_modes::run_mode_collect(
            &self.reg,
            text,
            &Spec::from(spec),
        ))
    }
}

impl Default for ModeRegistry {
    fn default() -> Self {
        ModeRegistry::new()
    }
}

/// Install a host clock: `f()` returns milliseconds since the epoch, or with no
/// argument `Date.now()`. A throw, a non-number or NaN makes that read unknown.
#[wasm_bindgen(js_name = installClock)]
pub fn install_clock(f: Option<Function>) {
    clock::install(f);
}

/// Remove the host clock (the time becomes unknown on wasm32-unknown-unknown).
#[wasm_bindgen(js_name = removeClock)]
pub fn remove_clock() {
    clock::remove();
}

/// 0 host clock, 1 system clock, 2 unavailable.
#[wasm_bindgen(js_name = clockStatus)]
pub fn clock_status() -> u32 {
    clock::status_code()
}

/// `cm_core::clock::now()`: milliseconds, NaN when unknown.
#[wasm_bindgen(js_name = clockNow)]
pub fn clock_now() -> f64 {
    cm_core::clock::now()
}

/// Reads that found no time.
#[wasm_bindgen(js_name = missingReads)]
pub fn missing_reads() -> f64 {
    cm_core::clock::missing_reads() as f64
}

/// Editors not yet dropped (created and not destroyed or collected).
#[wasm_bindgen(js_name = liveEditors)]
pub fn live_editors() -> u32 {
    LIVE.with(|l| l.get())
}

/// One editor.
#[wasm_bindgen]
pub struct WebEditor {
    inner: Option<Rc<RefCell<Inner>>>,
}

impl WebEditor {
    fn from_app(app: App) -> WebEditor {
        let inner = Rc::new(RefCell::new(Inner::new(app)));
        WebEditor { inner: Some(inner) }
    }

    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> Result<R, JsValue> {
        let rc = self.inner.as_ref().ok_or_else(destroyed)?;
        let mut inner = rc.try_borrow_mut().map_err(|_| busy())?;
        let r = f(&mut inner);
        // A direct API call is outside any browser event: deliver what it queued
        // (`fireOrphanDelayed`) and redraw.
        inner.session.app.deliver_pending();
        inner.schedule_frame();
        Ok(r)
    }

    fn from_parts(
        text: &str,
        mode: &str,
        options: &JsValue,
        registry: Option<Rc<Registry>>,
    ) -> WebEditor {
        let (eo, metrics) = editor_options(options);
        let mut app = App::new(
            text,
            None,
            metrics.clone(),
            &app_options(&eo),
            Box::new(EventClipboard::default()),
        );
        let editor = match registry {
            Some(reg) => Editor::with_registry(text, Spec::from(mode), eo, metrics, reg),
            None => Editor::new(text, Spec::from(mode), eo, metrics),
        };
        replace_editor(&mut app, editor);
        WebEditor::from_app(app)
    }
}

/// `App::new` builds its editor from `AppOptions`; the adapter takes all editor
/// options, so it swaps in an editor built from them and keeps the app's `save`
/// command.
fn replace_editor(app: &mut App, mut editor: Editor) {
    if let Some(save) = app.editor.commands.get("save").cloned() {
        editor.commands.insert("save".into(), save);
    }
    editor.doc.clear_history();
    editor.doc.mark_clean();
    app.editor = editor;
}

#[wasm_bindgen]
impl WebEditor {
    /// A headless editor for `text` in `mode` (a MIME type or mode name) with every
    /// ported mode. [`WebEditor::attach`] shows it in a page.
    #[wasm_bindgen(constructor)]
    pub fn new(text: &str, mode: &str, options: JsValue) -> WebEditor {
        WebEditor::from_parts(text, mode, &options, None)
    }

    /// An editor over the modes of `registry` (copied at creation).
    #[wasm_bindgen(js_name = withRegistry)]
    pub fn with_registry(
        text: &str,
        mode: &str,
        registry: &ModeRegistry,
        options: JsValue,
    ) -> WebEditor {
        WebEditor::from_parts(text, mode, &options, Some(Rc::new(registry.reg.clone())))
    }

    /// Open `path` through `files = {read(path), write(path, bytes)}` (see
    /// `files::JsFiles`); the mode comes from the file name unless `options.mode`
    /// names one. Decoding (UTF-8/Latin-1, gzip, line separators) is the shared
    /// `cm_view::fileio`. A failed read throws.
    pub fn open(path: &str, files: JsValue, options: JsValue) -> Result<WebEditor, JsValue> {
        let (eo, metrics) = editor_options(&options);
        let mut ao = app_options(&eo);
        ao.mode = opt(&options, "mode", |v| v.as_string());
        let app = App::open_with(
            Box::new(files::JsFiles::from_object(&files)),
            Path::new(path),
            metrics,
            &ao,
            Box::new(EventClipboard::default()),
        )
        .map_err(|e| JsValue::from(js_sys::Error::new(&e.to_string())))?;
        let w = WebEditor::from_app(app);
        w.with(|i| {
            let ed = i.editor();
            ed.options.tab_size = eo.tab_size;
            ed.options.indent_unit = eo.indent_unit;
            ed.options.indent_with_tabs = eo.indent_with_tabs;
            ed.options.mac = eo.mac;
            ed.options.line_wise_copy_cut = eo.line_wise_copy_cut;
        })?;
        Ok(w)
    }

    /// Show the editor inside `parent`: build its elements, measure the font,
    /// connect the browser events, and install `Date.now()` as the clock when no
    /// host clock is installed.
    pub fn attach(&self, parent: &web_sys::HtmlElement) -> Result<(), JsValue> {
        let rc = self.inner.as_ref().ok_or_else(destroyed)?;
        clock::ensure_installed();
        let weak = Rc::downgrade(rc);
        let mut inner = rc.try_borrow_mut().map_err(|_| busy())?;
        if inner.dom.is_some() {
            return Err(js_sys::Error::new("cm-web: already attached").into());
        }
        let (mut dom, metrics) = Dom::create(parent)?;
        dom.connect(&weak)?;
        let (w, h) = dom.client_size();
        inner.dom = Some(dom);
        inner.editor().set_metrics(metrics);
        // A new display starts with an empty view (`new Display`): the range a
        // headless editor kept for its default 800x600 size does not carry over.
        let first = inner.session.app.editor.first_line();
        let display = &mut inner.editor().display;
        display.view_from = first;
        display.view_to = first;
        inner.session.handle(Input::Resize {
            width: w,
            height: h,
        });
        inner.render();
        if let Some(d) = inner.dom.as_mut() {
            d.start_worker(100);
        }
        Ok(())
    }

    /// `refresh()` (src/edit/methods.js:411-421): measure the font again
    /// (`clearCaches`) and redraw everything. Call it after changing the font, or
    /// after showing an editor that was hidden when it last measured.
    pub fn refresh(&self) -> Result<(), JsValue> {
        self.with(|i| {
            let measured = i.dom.as_mut().and_then(|d| {
                d.clear_metric_cache();
                d.measure_uncached()
            });
            let m = measured.unwrap_or_else(|| i.session.app.editor.metrics.clone());
            i.editor().set_metrics(m);
            i.resized();
        })
    }

    /// Remove the editor's elements, handlers and timers, and drop the editor.
    /// Every later call throws.
    pub fn destroy(&mut self) -> Result<(), JsValue> {
        let rc = self.inner.take().ok_or_else(destroyed)?;
        rc.try_borrow_mut().map_err(|_| busy())?.detach();
        drop(rc);
        Ok(())
    }

    /// Whether [`WebEditor::destroy`] ran.
    #[wasm_bindgen(getter, js_name = isDestroyed)]
    pub fn is_destroyed(&self) -> bool {
        self.inner.is_none()
    }

    /// Draw the current state now instead of at the next animation frame.
    #[wasm_bindgen(js_name = renderNow)]
    pub fn render_now(&self) -> Result<(), JsValue> {
        self.with(|i| i.render())
    }

    /// Run the highlighter to the end of the document (tests, printing).
    #[wasm_bindgen(js_name = highlightAll)]
    pub fn highlight_all(&self) -> Result<(), JsValue> {
        self.with(|i| while i.session.app.idle() {})
    }

    /// Focus the input field (attached editors).
    pub fn focus(&self) -> Result<(), JsValue> {
        let field = self.with(|i| i.dom.as_ref().map(|d| d.field.clone()))?;
        if let Some(f) = field {
            f.focus()?;
        }
        Ok(())
    }

    #[wasm_bindgen(js_name = hasFocus)]
    pub fn has_focus(&self) -> Result<bool, JsValue> {
        self.with(|i| i.session.app.editor.has_focus())
    }

    #[wasm_bindgen(js_name = getValue)]
    pub fn get_value(&self) -> Result<String, JsValue> {
        self.with(|i| i.session.app.editor.get_value())
    }

    #[wasm_bindgen(js_name = setValue)]
    pub fn set_value(&self, text: &str) -> Result<(), JsValue> {
        self.with(|i| i.editor().set_value(text))
    }

    #[wasm_bindgen(js_name = getSelection)]
    pub fn get_selection(&self) -> Result<String, JsValue> {
        self.with(|i| i.session.app.editor.doc.get_selection(None))
    }

    /// `[line, ch]` of the primary head.
    #[wasm_bindgen(js_name = getCursor)]
    pub fn get_cursor(&self) -> Result<Array, JsValue> {
        self.with(|i| pos_array(i.session.app.editor.doc.get_cursor(None)))
    }

    /// `[[anchorLine, anchorCh, headLine, headCh], ...]`.
    #[wasm_bindgen(js_name = listSelections)]
    pub fn list_selections(&self) -> Result<Array, JsValue> {
        self.with(|i| {
            i.session
                .app
                .editor
                .list_selections()
                .into_iter()
                .map(|r| {
                    let a: Array = [r.anchor.line, r.anchor.ch, r.head.line, r.head.ch]
                        .iter()
                        .map(|v| JsValue::from(*v as f64))
                        .collect();
                    JsValue::from(a)
                })
                .collect()
        })
    }

    #[wasm_bindgen(js_name = setCursor)]
    pub fn set_cursor(&self, line: f64, ch: f64) -> Result<(), JsValue> {
        self.with(|i| {
            i.editor()
                .run_op(|cm| cm.doc.set_cursor(pos(line, ch), None))
        })
    }

    #[wasm_bindgen(js_name = setSelection)]
    pub fn set_selection(&self, al: f64, ac: f64, hl: f64, hc: f64) -> Result<(), JsValue> {
        self.with(|i| {
            i.editor()
                .run_op(|cm| cm.doc.set_selection(pos(al, ac), Some(pos(hl, hc)), None))
        })
    }

    /// `execCommand(name)`.
    #[wasm_bindgen(js_name = execCommand)]
    pub fn exec_command(&self, name: &str) -> Result<bool, JsValue> {
        self.with(|i| {
            let r = i.editor().exec_command(name);
            i.session.app.flush_save();
            r
        })
    }

    /// Typed text (`applyTextInput`, origin `+input`).
    #[wasm_bindgen(js_name = typeText)]
    pub fn type_text(&self, text: &str) -> Result<bool, JsValue> {
        self.with(|i| i.editor().type_text(text))
    }

    /// A key press by CM5 name (`"Ctrl-Z"`, `"Enter"`, `"Shift-Left"`) through the
    /// key maps; returns whether a binding handled it.
    #[wasm_bindgen(js_name = keyDown)]
    pub fn key_down(&self, name: &str) -> Result<bool, JsValue> {
        let key = cm_view::keymap::normalize_key_name(name)
            .map_err(|e| JsValue::from(js_sys::Error::new(&e)))?;
        self.with(|i| i.session.key_down(Some(KeyEvent::parse(&key)), false))
    }

    /// A DOM `keydown` by `keyCode`, `code` and modifiers (`keyName`, then the key
    /// maps); returns whether to cancel the default action.
    #[wasm_bindgen(js_name = domKeyDown)]
    pub fn dom_key_down(
        &self,
        key_code: u32,
        code: &str,
        shift: bool,
        ctrl: bool,
        alt: bool,
        meta: bool,
    ) -> Result<bool, JsValue> {
        let key = cm_view::keymap::key_event_from_dom(
            key_code,
            Some(code),
            [shift, ctrl, alt, meta],
            false,
        );
        self.with(|i| i.session.key_down(key, false))
    }

    /// `indentLine(n, how)`: `"smart"`, `"prev"`, `"add"`, `"subtract"`.
    #[wasm_bindgen(js_name = indentLine)]
    pub fn indent_line(&self, line: f64, how: &str) -> Result<(), JsValue> {
        let how = match how {
            "smart" => Indent::Smart,
            "prev" => Indent::Prev,
            "add" => Indent::Add,
            "subtract" => Indent::Subtract,
            _ => return Err(js_sys::Error::new("cm-web: unknown indent kind").into()),
        };
        self.with(|i| i.editor().indent_line(line as i64, Some(how), true))
    }

    /// `historySize()`: `[undo, redo]`.
    #[wasm_bindgen(js_name = historySize)]
    pub fn history_size(&self) -> Result<Array, JsValue> {
        self.with(|i| {
            let h = i.session.app.editor.doc.history_size();
            Array::of2(&JsValue::from(h.undo as f64), &JsValue::from(h.redo as f64))
        })
    }

    /// `addLineClass(line, where, cls)`. An invalid class pattern throws a `SyntaxError`, as
    /// in CM5 (the regular expression is built from `cls` unescaped).
    #[wasm_bindgen(js_name = addLineClass)]
    pub fn add_line_class(&self, line: f64, place: &str, cls: &str) -> Result<(), JsValue> {
        let place = LineWhere::parse(place);
        self.with(|i| {
            i.session
                .app
                .editor
                .doc
                .add_line_class(line as i64, place, cls)
        })?
        .map(|_| ())
        .map_err(|e| js_sys::SyntaxError::new(&e.0).into())
    }

    /// `removeLineClass(line, where, cls)`; a `null` or `undefined` class clears the property.
    #[wasm_bindgen(js_name = removeLineClass)]
    pub fn remove_line_class(
        &self,
        line: f64,
        place: &str,
        cls: Option<String>,
    ) -> Result<(), JsValue> {
        let place = LineWhere::parse(place);
        self.with(|i| {
            i.session
                .app
                .editor
                .doc
                .remove_line_class(line as i64, place, cls.as_deref())
        })?
        .map(|_| ())
        .map_err(|e| js_sys::SyntaxError::new(&e.0).into())
    }

    /// The class properties of `lineInfo(line)`: `[textClass, bgClass, wrapClass, gutterClass]`,
    /// each a string or `null` (CM5 also has `undefined` for a never-set property).
    #[wasm_bindgen(js_name = lineClasses)]
    pub fn line_classes(&self, line: f64) -> Result<Array, JsValue> {
        self.with(|i| {
            let h = i.session.app.editor.doc.get_line_handle(line as i64);
            [
                h.as_ref().and_then(|h| h.text_class()),
                h.as_ref().and_then(|h| h.bg_class()),
                h.as_ref().and_then(|h| h.wrap_class()),
                h.as_ref().and_then(|h| h.gutter_class()),
            ]
            .into_iter()
            .map(|c| c.map_or(JsValue::NULL, |c| JsValue::from_str(&c)))
            .collect()
        })
    }

    #[wasm_bindgen(js_name = isClean)]
    pub fn is_clean(&self) -> Result<bool, JsValue> {
        self.with(|i| i.session.app.editor.doc.is_clean(None))
    }

    /// `getMode().name`.
    #[wasm_bindgen(js_name = modeName)]
    pub fn mode_name(&self) -> Result<String, JsValue> {
        self.with(|i| i.session.app.editor.mode_name())
    }

    /// `setOption("mode", spec)`.
    #[wasm_bindgen(js_name = setMode)]
    pub fn set_mode(&self, spec: &str) -> Result<(), JsValue> {
        self.with(|i| {
            i.editor().set_mode(Spec::from(spec));
            if let Some(d) = i.dom.as_mut() {
                d.start_worker(100);
            }
        })
    }

    /// `getLineTokens(line, true)`: `[[text, type|null], ...]`.
    #[wasm_bindgen(js_name = lineTokens)]
    pub fn line_tokens(&self, line: f64) -> Result<Array, JsValue> {
        self.with(|i| {
            tokens_array(
                i.session
                    .app
                    .editor
                    .get_line_tokens(line as i64, true)
                    .into_iter()
                    .map(|t| (t.string, t.ty)),
            )
        })
    }

    /// The copy event's text (`prepareCopyCut`), or `undefined` when nothing would
    /// be copied.
    pub fn copy(&self) -> Result<Option<String>, JsValue> {
        self.with(|i| i.session.copy())
    }

    /// The cut event: the text, and the selection is deleted.
    pub fn cut(&self) -> Result<Option<String>, JsValue> {
        self.with(|i| i.session.cut())
    }

    /// The paste event with `text` (`handlePaste`, origin `paste`).
    pub fn paste(&self, text: &str) -> Result<(), JsValue> {
        self.with(|i| i.session.paste(text))
    }

    /// Read the system clipboard (`navigator.clipboard.readText`) and paste it.
    /// The promise resolves to true, or rejects with `clipboard-unavailable`
    /// (no async clipboard API) or `clipboard-denied: <reason>` (no permission,
    /// no user gesture, an insecure context). Some browsers ask the user first
    /// (Firefox shows a paste prompt) and settle only when they answer; with
    /// `timeout_ms` the promise rejects with `clipboard-timeout` instead, and a
    /// late answer pastes nothing.
    #[wasm_bindgen(js_name = pasteFromSystem)]
    pub fn paste_from_system(&self, timeout_ms: Option<i32>) -> js_sys::Promise {
        let Some(rc) = self.inner.clone() else {
            return js_sys::Promise::reject(&destroyed());
        };
        let Some(read) = system_clipboard("readText") else {
            return js_sys::Promise::reject(&JsValue::from_str("clipboard-unavailable"));
        };
        let read = with_timeout(read, timeout_ms);
        let weak = Rc::downgrade(&rc);
        let ok = Closure::once_into_js(move |text: JsValue| -> JsValue {
            let Some(rc) = weak.upgrade() else {
                return destroyed();
            };
            let Ok(mut inner) = rc.try_borrow_mut() else {
                return busy();
            };
            inner.session.paste(&text.as_string().unwrap_or_default());
            inner.after_input(400);
            JsValue::TRUE
        });
        then2(&read, &ok.unchecked_into(), &denied())
    }

    /// Write the current selection (as the copy event would) to the system
    /// clipboard (`navigator.clipboard.writeText`). Resolves to the text written;
    /// rejects like [`WebEditor::paste_from_system`], or with `nothing-to-copy`.
    #[wasm_bindgen(js_name = copyToSystem)]
    pub fn copy_to_system(&self, timeout_ms: Option<i32>) -> js_sys::Promise {
        let text = match self.with(|i| i.session.copy()) {
            Ok(Some(t)) => t,
            Ok(None) => return js_sys::Promise::reject(&JsValue::from_str("nothing-to-copy")),
            Err(e) => return js_sys::Promise::reject(&e),
        };
        let Some(write) = system_clipboard_call("writeText", &text) else {
            return js_sys::Promise::reject(&JsValue::from_str("clipboard-unavailable"));
        };
        let write = with_timeout(write, timeout_ms);
        let t = JsValue::from_str(&text);
        let ok = Closure::once_into_js(move |_: JsValue| -> JsValue { t });
        then2(&write, &ok.unchecked_into(), &denied())
    }

    /// Save through the file host: encode in shared Rust, `write(path, bytes)`,
    /// mark clean. Throws (and keeps the document dirty) when the write fails.
    pub fn save(&self) -> Result<(), JsValue> {
        self.with(|i| i.session.app.save())?
            .map_err(|e| JsValue::from(js_sys::Error::new(&e.to_string())))
    }

    /// The status line (`saved …`, `save failed: …`, line-ending notes).
    pub fn status(&self) -> Result<String, JsValue> {
        self.with(|i| i.session.app.status.clone())
    }

    /// `scrollTo(x, y)`.
    #[wasm_bindgen(js_name = scrollTo)]
    pub fn scroll_to(&self, x: Option<f64>, y: Option<f64>) -> Result<(), JsValue> {
        self.with(|i| i.editor().scroll_to(x, y))
    }

    /// `getScrollInfo()`: `{left, top, width, height, clientWidth, clientHeight,
    /// viewFrom, viewTo}`.
    #[wasm_bindgen(js_name = scrollInfo)]
    pub fn scroll_info(&self) -> Result<Object, JsValue> {
        self.with(|i| {
            let ed = &i.session.app.editor;
            let o = Object::new();
            let (mh, mw) = ed.max_scroll();
            let set = |k: &str, v: f64| {
                let _ = Reflect::set(&o, &JsValue::from_str(k), &JsValue::from(v));
            };
            set("left", ed.display.scroll_left);
            set("top", ed.display.scroll_top);
            set("height", ed.doc_height() + 2.0 * ed.metrics.padding_top);
            set("width", mw + ed.text_area_width());
            set("clientWidth", ed.display.width);
            set("clientHeight", ed.display.height);
            set("maxTop", mh);
            set("viewFrom", ed.display.view_from as f64);
            set("viewTo", ed.display.view_to as f64);
            o
        })
    }

    /// `cursorCoords(pos, "local")`: `{left, top, bottom}` relative to the sizer.
    #[wasm_bindgen(js_name = cursorCoords)]
    pub fn cursor_coords(&self, line: f64, ch: f64) -> Result<Object, JsValue> {
        self.with(|i| {
            let r = i
                .session
                .app
                .editor
                .cursor_coords(pos(line, ch), cm_view::CoordSys::Local);
            let o = Object::new();
            for (k, v) in [("left", r.left), ("top", r.top), ("bottom", r.bottom)] {
                let _ = Reflect::set(&o, &JsValue::from_str(k), &JsValue::from(v));
            }
            o
        })
    }

    /// `{charWidth, textHeight, wideWidth, astralWidth, gutterWidth}` in use.
    pub fn metrics(&self) -> Result<Object, JsValue> {
        self.with(|i| {
            let ed = &i.session.app.editor;
            let m = &ed.metrics;
            let o = Object::new();
            for (k, v) in [
                ("charWidth", m.char_width),
                ("textHeight", m.text_height),
                ("wideWidth", m.wide_width),
                ("astralWidth", m.astral_width),
                ("gutterWidth", ed.gutter_width),
            ] {
                let _ = Reflect::set(&o, &JsValue::from_str(k), &JsValue::from(v));
            }
            o
        })
    }

    /// Adapter counters: `{frames, renderedLines, pendingTimers, blinkTimer,
    /// workTimer, frameRequest, ignoredInputs, highlightFrontier, lineCount}`.
    pub fn stats(&self) -> Result<Object, JsValue> {
        let rc = self.inner.as_ref().ok_or_else(destroyed)?;
        let i = rc.try_borrow().map_err(|_| busy())?;
        let o = Object::new();
        let d = i.dom.as_ref();
        for (k, v) in [
            ("frames", d.map_or(0, |d| d.frames) as f64),
            ("renderedLines", d.map_or(0, |d| d.rendered_lines()) as f64),
            ("pendingTimers", d.map_or(0, |d| d.pending_timers()) as f64),
            ("blinkTimer", d.is_some_and(|d| d.timers().0) as u8 as f64),
            ("workTimer", d.is_some_and(|d| d.timers().1) as u8 as f64),
            ("frameRequest", d.is_some_and(|d| d.timers().2) as u8 as f64),
            ("ignoredInputs", i.session.ignored.len() as f64),
            (
                "highlightFrontier",
                i.session.app.editor.hs.borrow().highlight_frontier as f64,
            ),
            ("lineCount", i.session.app.editor.doc.line_count() as f64),
        ] {
            let _ = Reflect::set(&o, &JsValue::from_str(k), &JsValue::from(v));
        }
        let mac = JsValue::from(i.session.app.editor.options.mac);
        let _ = Reflect::set(&o, &JsValue::from_str("mac"), &mac);
        Ok(o)
    }

    /// `cursorBlinkRate` in ms (0: no blinking, negative: hidden cursor).
    #[wasm_bindgen(js_name = setCursorBlinkRate)]
    pub fn set_cursor_blink_rate(&self, ms: i32) -> Result<(), JsValue> {
        self.with(|i| {
            if let Some(d) = i.dom.as_mut() {
                d.blink_rate = ms;
            }
            i.restart_blink();
        })
    }
}

fn system_clipboard_object() -> Option<JsValue> {
    let nav = web_sys::window()?.navigator();
    let c = Reflect::get(&nav, &JsValue::from_str("clipboard")).ok()?;
    (!c.is_undefined() && !c.is_null()).then_some(c)
}

fn system_clipboard(method: &str) -> Option<js_sys::Promise> {
    let c = system_clipboard_object()?;
    let f: Function = Reflect::get(&c, &JsValue::from_str(method))
        .ok()?
        .dyn_into()
        .ok()?;
    Some(match f.call0(&c) {
        Ok(p) => p
            .dyn_into()
            .unwrap_or_else(|v| js_sys::Promise::resolve(&v)),
        Err(e) => js_sys::Promise::reject(&e),
    })
}

fn system_clipboard_call(method: &str, arg: &str) -> Option<js_sys::Promise> {
    let c = system_clipboard_object()?;
    let f: Function = Reflect::get(&c, &JsValue::from_str(method))
        .ok()?
        .dyn_into()
        .ok()?;
    Some(match f.call1(&c, &JsValue::from_str(arg)) {
        Ok(p) => p
            .dyn_into()
            .unwrap_or_else(|v| js_sys::Promise::resolve(&v)),
        Err(e) => js_sys::Promise::reject(&e),
    })
}

/// `p`, or a rejection with `clipboard-timeout` after `ms` if `p` is still pending.
fn with_timeout(p: js_sys::Promise, ms: Option<i32>) -> js_sys::Promise {
    let (Some(ms), Some(window)) = (ms, web_sys::window()) else {
        return p;
    };
    let timer = js_sys::Promise::new(&mut |_resolve, reject| {
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_1(
            &reject,
            ms,
            &JsValue::from_str("clipboard-timeout"),
        );
    });
    js_sys::Promise::race(&Array::of2(&p, &timer))
}

/// `p.then(ok, err)` with plain functions.
fn then2(p: &js_sys::Promise, ok: &Function, err: &Function) -> js_sys::Promise {
    let then = Reflect::get(p, &JsValue::from_str("then"))
        .ok()
        .and_then(|f| f.dyn_into::<Function>().ok());
    match then.map(|f| f.call2(p, ok, err)) {
        Some(Ok(v)) => v.unchecked_into(),
        Some(Err(e)) => js_sys::Promise::reject(&e),
        None => js_sys::Promise::reject(&JsValue::from_str("cm-web: not a promise")),
    }
}

/// The rejection handler of the clipboard promises (called at most once).
fn denied() -> Function {
    Closure::once_into_js(|e: JsValue| -> JsValue {
        if e.as_string()
            .is_some_and(|s| s.starts_with("cm-web") || s == "clipboard-timeout")
        {
            return js_sys::Promise::reject(&e).into();
        }
        let msg = e
            .dyn_ref::<js_sys::Error>()
            .map(|e| String::from(e.message()))
            .or_else(|| e.as_string())
            .unwrap_or_default();
        js_sys::Promise::reject(&JsValue::from_str(&format!("clipboard-denied: {msg}"))).into()
    })
    .unchecked_into()
}
