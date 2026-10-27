//! The DOM side: elements, browser events, font measurement and scheduling.
//!
//! The element tree follows CM5's (`src/display/Display.js`), reduced to what a
//! frame needs. Containers have adapter class names, so `codemirror.css` on the
//! same page (its scrollbar-hiding margins and paddings) does not change them;
//! line, token, cursor and selection elements keep CM5's names for themes:
//!
//! ```text
//! div.CodeMirror.cm-web            root, owned by the adapter
//!   div.cm-web-field > textarea    hidden input field (TextareaInput), at the cursor
//!   div.cm-web-scroll              the scroller (native scrollbars)
//!     div.cm-web-sizer             content size; margin-left = gutter width
//!       div.cm-web-lines           origin of "div" coordinates (top = paddingTop)
//!         div (selection boxes)
//!         div.CodeMirror-code      pre.CodeMirror-line per rendered line
//!         div.CodeMirror-cursors
//!         span.cm-web-preedit      IME preedit (display only)
//!         div.cm-web-measure
//!     div.cm-web-gutters           line numbers, kept at scrollLeft
//! ```
//!
//! Every handler borrows the editor state once, applies the event through
//! [`crate::session::Session`], and does DOM work that can fire events
//! synchronously (`focus()`) only after the borrow ends.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use cm_view::Metrics;
use cm_view::app::Input;
use cm_view::keymap::key_event_from_dom;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    AddEventListenerOptions, ClipboardEvent, CompositionEvent, Document, Element, Event,
    EventTarget, HtmlElement, HtmlTextAreaElement, InputEvent, KeyboardEvent, MouseEvent,
    ResizeObserver, TouchEvent, WheelEvent, Window,
};

use crate::Inner;
use crate::frame::{self, Frame, Seg};
use crate::session::BeforeInput;

/// `codemirror.css` (5.65.21) reduced to the adapter's elements, with the default
/// theme's token colors (`.cm-s-default`). The root's font, size and colors are
/// defaults without specificity (`:where`), so page CSS such as
/// `.CodeMirror { height: 100% }` overrides them, as it does for CM5.
pub const STYLE: &str = r#"
:where(.cm-web.CodeMirror){font-family:monospace;height:300px;color:black;background:white}
.cm-web.CodeMirror{position:relative;overflow:hidden;direction:ltr}
.cm-web .cm-web-scroll{position:absolute;left:0;top:0;right:0;bottom:0;overflow:auto;outline:none}
.cm-web .cm-web-sizer{position:relative;box-sizing:content-box}
.cm-web .cm-web-lines{position:absolute;left:0;cursor:text}
.cm-web .CodeMirror-code{position:absolute;left:0;right:0;z-index:2}
.cm-web pre.CodeMirror-line{margin:0;padding:0 4px;border-radius:0;border-width:0;background:transparent;font-family:inherit;font-size:inherit;white-space:pre;word-wrap:normal;line-height:inherit;color:inherit;position:relative;overflow:visible;-webkit-tap-highlight-color:transparent;font-variant-ligatures:contextual;box-sizing:content-box}
.cm-web.CodeMirror-wrap pre.CodeMirror-line{white-space:pre-wrap;word-wrap:break-word;word-break:normal}
.cm-web .CodeMirror-selected{position:absolute;background:#d9d9d9;z-index:1}
.cm-web.CodeMirror-focused .CodeMirror-selected{background:#d7d4f0}
.cm-web .CodeMirror-cursors{position:absolute;left:0;top:0;visibility:hidden;z-index:3;pointer-events:none}
.cm-web.CodeMirror-focused .CodeMirror-cursors{visibility:visible}
.cm-web .CodeMirror-cursor{position:absolute;border-left:1px solid black;border-right:none;width:0;pointer-events:none}
.cm-web .CodeMirror-overwrite .CodeMirror-cursor{border-left:none;background:#7e7}
.cm-web .cm-web-gutters{position:absolute;left:0;top:0;z-index:3;background:#f7f7f7;border-right:1px solid #ddd;box-sizing:content-box}
.cm-web .CodeMirror-linenumber{position:absolute;right:0;padding:0 3px 0 5px;min-width:20px;text-align:right;color:#999;white-space:nowrap;box-sizing:content-box}
.cm-web .cm-web-field{position:absolute;overflow:hidden;width:3px;height:0;z-index:0}
.cm-web .cm-web-field textarea{position:absolute;bottom:-1em;padding:0;width:1000px;height:1em;min-height:1em;outline:none;border:none;resize:none;overflow:hidden}
.cm-web .cm-web-preedit{position:absolute;z-index:4;white-space:pre;text-decoration:underline;background:white;pointer-events:none}
.cm-web .cm-web-measure{position:absolute;left:0;top:0;width:100%;height:0;overflow:hidden;visibility:hidden}
.cm-web .cm-tab{display:inline-block;text-decoration:inherit}
.cm-s-default .cm-header{color:blue}.cm-s-default .cm-quote{color:#090}.cm-negative{color:#d44}.cm-positive{color:#292}
.cm-header,.cm-strong{font-weight:bold}.cm-em{font-style:italic}.cm-link{text-decoration:underline}.cm-strikethrough{text-decoration:line-through}
.cm-s-default .cm-keyword{color:#708}.cm-s-default .cm-atom{color:#219}.cm-s-default .cm-number{color:#164}.cm-s-default .cm-def{color:#00f}
.cm-s-default .cm-variable-2{color:#05a}.cm-s-default .cm-variable-3,.cm-s-default .cm-type{color:#085}.cm-s-default .cm-comment{color:#a50}
.cm-s-default .cm-string{color:#a11}.cm-s-default .cm-string-2{color:#f50}.cm-s-default .cm-meta{color:#555}.cm-s-default .cm-qualifier{color:#555}
.cm-s-default .cm-builtin{color:#30a}.cm-s-default .cm-bracket{color:#997}.cm-s-default .cm-tag{color:#170}.cm-s-default .cm-attribute{color:#00c}
.cm-s-default .cm-hr{color:#999}.cm-s-default .cm-link{color:#00c}.cm-s-default .cm-error{color:#f00}.cm-invalidchar{color:red}
"#;

/// CM5 option defaults the adapter schedules with (src/edit/options.js).
pub const CURSOR_BLINK_RATE: i32 = 530;
/// `workTime` and `workDelay` (src/display/highlight_worker.js).
pub const WORK_TIME: f64 = 100.0;
pub const WORK_DELAY: i32 = 100;

type Handler<E> = fn(&mut Inner, &E) -> Post;
type Listener = (EventTarget, &'static str, Closure<dyn FnMut(Event)>);
type Observer = (ResizeObserver, Closure<dyn FnMut(JsValue)>);

/// Work a handler leaves for after its borrow.
#[derive(Default)]
pub struct Post {
    focus: bool,
}

struct CachedLine {
    text: Vec<u16>,
    styles: Rc<cm_view::highlight::Styles>,
    wrap: Option<f64>,
    tab_size: i64,
    el: HtmlElement,
}

struct Touch {
    x: f64,
    y: f64,
    time: f64,
    moved: bool,
}

pub struct Dom {
    window: Window,
    document: Document,
    pub root: HtmlElement,
    pub field: HtmlTextAreaElement,
    field_wrap: HtmlElement,
    pub scroller: HtmlElement,
    sizer: HtmlElement,
    lines_div: HtmlElement,
    code: HtmlElement,
    sel_div: HtmlElement,
    cursors_div: HtmlElement,
    preedit: HtmlElement,
    gutter: HtmlElement,
    cache: HashMap<usize, CachedLine>,
    order: Vec<usize>,
    numbers_key: Vec<(String, i64)>,
    listeners: Vec<Listener>,
    observer: Option<Observer>,
    raf: Option<i32>,
    raf_cb: Option<Closure<dyn FnMut()>>,
    blink: Option<i32>,
    blink_on: bool,
    blink_cb: Option<Closure<dyn FnMut()>>,
    work: Option<(i32, f64)>,
    work_cb: Option<Closure<dyn FnMut()>>,
    dragging: bool,
    touch: Option<Touch>,
    pub frames: u32,
    pub blink_rate: i32,
}

fn el(document: &Document, tag: &str, class: &str) -> Result<HtmlElement, JsValue> {
    let e: HtmlElement = document.create_element(tag)?.dyn_into()?;
    if !class.is_empty() {
        e.set_class_name(class);
    }
    Ok(e)
}

fn css(e: &HtmlElement, prop: &str, value: &str) {
    let _ = e.style().set_property(prop, value);
}

fn px(v: f64) -> String {
    format!("{v}px")
}

/// Measure the font of `lines` the way `charWidth`/`textHeight` do
/// (src/measurement/position_measurement.js), plus one wide (CJK) and one astral
/// character for the box model.
fn measure(document: &Document, lines: &HtmlElement) -> Result<Metrics, JsValue> {
    let m = el(document, "div", "cm-web-measure")?;
    lines.append_child(&m)?;
    let width_of = |text: &str| -> Result<f64, JsValue> {
        let pre = el(document, "pre", "CodeMirror-line")?;
        let span = el(document, "span", "")?;
        span.set_text_content(Some(text));
        pre.append_child(&span)?;
        m.append_child(&pre)?;
        Ok(span.get_bounding_client_rect().width() / 10.0)
    };
    let char_width = width_of("xxxxxxxxxx")?;
    let wide = width_of(&"\u{65e5}".repeat(10))?;
    let astral = width_of(&"\u{1f600}".repeat(10))?;
    // textHeight: 49 "x" + <br> and a last "x" in one pre, height / 50.
    let pre = el(document, "pre", "CodeMirror-line")?;
    for _ in 0..49 {
        pre.append_child(&document.create_text_node("x"))?;
        let br: web_sys::Node = document.create_element("br")?.into();
        pre.append_child(&br)?;
    }
    pre.append_child(&document.create_text_node("x"))?;
    m.append_child(&pre)?;
    let text_height = pre.get_bounding_client_rect().height() / 50.0;
    m.remove();
    let mut metrics = Metrics::monospace(
        if char_width > 2.0 { char_width } else { 10.0 },
        if text_height > 3.0 { text_height } else { 15.0 },
    );
    if wide > 0.0 {
        metrics.wide_width = wide;
    }
    if astral > 0.0 {
        metrics.astral_width = astral;
    }
    Ok(metrics)
}

impl Dom {
    /// Build the element tree inside `parent` and measure its font.
    pub fn create(parent: &HtmlElement) -> Result<(Dom, Metrics), JsValue> {
        let window = web_sys::window().ok_or("cm-web: no window")?;
        let document = window.document().ok_or("cm-web: no document")?;
        if document.get_element_by_id("cm-web-style").is_none() {
            let style = document.create_element("style")?;
            style.set_id("cm-web-style");
            style.set_text_content(Some(STYLE));
            let head = document
                .head()
                .map(|h| h.unchecked_into::<Element>())
                .or_else(|| document.document_element())
                .ok_or("cm-web: no document element")?;
            head.append_child(&style)?;
        }
        let root = el(&document, "div", "CodeMirror cm-web cm-s-default")?;
        let field_wrap = el(&document, "div", "cm-web-field")?;
        let field: HtmlTextAreaElement = document.create_element("textarea")?.dyn_into()?;
        // disableBrowserMagic + hiddenTextarea (src/input/input.js).
        for (k, v) in [
            ("autocorrect", "off"),
            ("autocapitalize", "off"),
            ("spellcheck", "false"),
            ("tabindex", "0"),
            ("wrap", "off"),
        ] {
            field.set_attribute(k, v)?;
        }
        field_wrap.append_child(&field)?;
        let scroller = el(&document, "div", "cm-web-scroll")?;
        scroller.set_attribute("tabindex", "-1")?;
        let sizer = el(&document, "div", "cm-web-sizer")?;
        let lines_div = el(&document, "div", "cm-web-lines")?;
        lines_div.set_attribute("role", "presentation")?;
        let sel_div = el(&document, "div", "")?;
        let code = el(&document, "div", "CodeMirror-code")?;
        code.set_attribute("role", "presentation")?;
        let cursors_div = el(&document, "div", "CodeMirror-cursors")?;
        let preedit = el(&document, "span", "cm-web-preedit")?;
        css(&preedit, "display", "none");
        let gutter = el(&document, "div", "cm-web-gutters")?;
        for c in [&sel_div, &code, &cursors_div] {
            lines_div.append_child(c)?;
        }
        lines_div.append_child(&preedit)?;
        sizer.append_child(&lines_div)?;
        scroller.append_child(&sizer)?;
        scroller.append_child(&gutter)?;
        root.append_child(&field_wrap)?;
        root.append_child(&scroller)?;
        parent.append_child(&root)?;
        let metrics = measure(&document, &lines_div)?;
        let dom = Dom {
            window,
            document,
            root,
            field,
            field_wrap,
            scroller,
            sizer,
            lines_div,
            code,
            sel_div,
            cursors_div,
            preedit,
            gutter,
            cache: HashMap::new(),
            order: Vec::new(),
            numbers_key: Vec::new(),
            listeners: Vec::new(),
            observer: None,
            raf: None,
            raf_cb: None,
            blink: None,
            blink_on: true,
            blink_cb: None,
            work: None,
            work_cb: None,
            dragging: false,
            touch: None,
            frames: 0,
            blink_rate: CURSOR_BLINK_RATE,
        };
        Ok((dom, metrics))
    }

    /// The scroller's client size: the editor's `display.width`/`height`.
    pub fn client_size(&self) -> (f64, f64) {
        (
            self.scroller.client_width() as f64,
            self.scroller.client_height() as f64,
        )
    }

    /// Register the event handlers and timers' callbacks. They hold only a weak
    /// reference to the editor state.
    pub fn connect(&mut self, weak: &Weak<RefCell<Inner>>) -> Result<(), JsValue> {
        let field: EventTarget = self.field.clone().into();
        let scroller: EventTarget = self.scroller.clone().into();
        let window: EventTarget = self.window.clone().into();
        self.listen::<KeyboardEvent>(&field, "keydown", None, weak, on_key_down)?;
        self.listen::<InputEvent>(&field, "beforeinput", None, weak, on_before_input)?;
        self.listen::<Event>(&field, "input", None, weak, on_input)?;
        self.listen::<CompositionEvent>(&field, "compositionstart", None, weak, on_comp_start)?;
        self.listen::<CompositionEvent>(&field, "compositionupdate", None, weak, on_comp_update)?;
        self.listen::<CompositionEvent>(&field, "compositionend", None, weak, on_comp_end)?;
        self.listen::<ClipboardEvent>(&field, "copy", None, weak, on_copy)?;
        self.listen::<ClipboardEvent>(&field, "cut", None, weak, on_cut)?;
        self.listen::<ClipboardEvent>(&field, "paste", None, weak, on_paste)?;
        self.listen::<Event>(&field, "focus", None, weak, on_focus)?;
        self.listen::<Event>(&field, "blur", None, weak, on_blur)?;
        self.listen::<MouseEvent>(&scroller, "mousedown", None, weak, on_mouse_down)?;
        self.listen::<WheelEvent>(&scroller, "wheel", Some(false), weak, on_wheel)?;
        self.listen::<Event>(&scroller, "scroll", Some(true), weak, on_scroll)?;
        self.listen::<TouchEvent>(&scroller, "touchstart", Some(true), weak, on_touch_start)?;
        self.listen::<TouchEvent>(&scroller, "touchmove", Some(true), weak, on_touch_move)?;
        self.listen::<TouchEvent>(&scroller, "touchend", Some(false), weak, on_touch_end)?;
        self.listen::<MouseEvent>(&window, "mousemove", None, weak, on_mouse_move)?;
        self.listen::<MouseEvent>(&window, "mouseup", None, weak, on_mouse_up)?;

        let w = weak.clone();
        self.raf_cb = Some(Closure::new(move || {
            with_inner(&w, |inner| {
                if let Some(d) = inner.dom.as_mut() {
                    d.raf = None;
                }
                inner.render();
                Post::default()
            })
        }));
        let w = weak.clone();
        self.blink_cb = Some(Closure::new(move || {
            with_inner(&w, |inner| {
                if let Some(d) = inner.dom.as_mut() {
                    d.blink_on = !d.blink_on;
                    let v = if d.blink_on { "" } else { "hidden" };
                    css(&d.cursors_div, "visibility", v);
                }
                Post::default()
            })
        }));
        let w = weak.clone();
        self.work_cb = Some(Closure::new(move || {
            with_inner(&w, |inner| {
                if let Some(d) = inner.dom.as_mut() {
                    d.work = None;
                }
                inner.highlight_slice();
                Post::default()
            })
        }));
        let w = weak.clone();
        let ro_cb: Closure<dyn FnMut(JsValue)> = Closure::new(move |_entries: JsValue| {
            with_inner(&w, |inner| {
                inner.resized();
                Post::default()
            })
        });
        let ro = ResizeObserver::new(ro_cb.as_ref().unchecked_ref())?;
        ro.observe(&self.scroller);
        self.observer = Some((ro, ro_cb));
        Ok(())
    }

    fn listen<E: JsCast + 'static>(
        &mut self,
        target: &EventTarget,
        name: &'static str,
        passive: Option<bool>,
        weak: &Weak<RefCell<Inner>>,
        f: Handler<E>,
    ) -> Result<(), JsValue> {
        let w = weak.clone();
        let cb: Closure<dyn FnMut(Event)> = Closure::new(move |ev: Event| {
            with_inner(&w, |inner| f(inner, ev.unchecked_ref::<E>()))
        });
        let opts = AddEventListenerOptions::new();
        if let Some(p) = passive {
            opts.set_passive(p);
        }
        target.add_event_listener_with_callback_and_add_event_listener_options(
            name,
            cb.as_ref().unchecked_ref(),
            &opts,
        )?;
        self.listeners.push((target.clone(), name, cb));
        Ok(())
    }

    /// Remove every handler, timer and observer, then the elements.
    pub fn teardown(&mut self) {
        for (t, name, cb) in self.listeners.drain(..) {
            let _ = t.remove_event_listener_with_callback(name, cb.as_ref().unchecked_ref());
        }
        if let Some((ro, _cb)) = self.observer.take() {
            ro.disconnect();
        }
        if let Some(id) = self.raf.take() {
            let _ = self.window.cancel_animation_frame(id);
        }
        if let Some(id) = self.blink.take() {
            self.window.clear_interval_with_handle(id);
        }
        if let Some((id, _)) = self.work.take() {
            self.window.clear_timeout_with_handle(id);
        }
        self.cache.clear();
        self.root.remove();
    }

    /// Timers and frame requests still armed.
    pub fn pending_timers(&self) -> u32 {
        self.raf.is_some() as u32 + self.blink.is_some() as u32 + self.work.is_some() as u32
    }

    /// Whether the blink interval, the highlight worker timeout and an animation
    /// frame request are armed.
    pub fn timers(&self) -> (bool, bool, bool) {
        (
            self.blink.is_some(),
            self.work.is_some(),
            self.raf.is_some(),
        )
    }

    pub fn schedule_frame(&mut self) {
        if self.raf.is_some() {
            return;
        }
        if let Some(cb) = &self.raf_cb {
            self.raf = self
                .window
                .request_animation_frame(cb.as_ref().unchecked_ref())
                .ok();
        }
    }

    /// `restartBlink`: show the cursor and toggle it every `cursorBlinkRate` ms
    /// while focused.
    pub fn restart_blink(&mut self, focused: bool) {
        if let Some(id) = self.blink.take() {
            self.window.clear_interval_with_handle(id);
        }
        self.blink_on = true;
        css(&self.cursors_div, "visibility", "");
        if !focused {
            return;
        }
        if self.blink_rate > 0 {
            if let Some(cb) = &self.blink_cb {
                self.blink = self
                    .window
                    .set_interval_with_callback_and_timeout_and_arguments_0(
                        cb.as_ref().unchecked_ref(),
                        self.blink_rate,
                    )
                    .ok();
            }
        } else if self.blink_rate < 0 {
            css(&self.cursors_div, "visibility", "hidden");
        }
    }

    /// `startWorker(cm, time)` (`Delayed.set`: keep the earlier deadline).
    pub fn start_worker(&mut self, ms: i32) {
        let at = js_sys::Date::now() + ms as f64;
        if let Some((id, t)) = self.work {
            if t <= at {
                return;
            }
            self.window.clear_timeout_with_handle(id);
            self.work = None;
        }
        if let Some(cb) = &self.work_cb
            && let Ok(id) = self
                .window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    ms,
                )
        {
            self.work = Some((id, at));
        }
    }

    fn build_line(&self, line: &frame::FrameLine, tab_size: i64) -> Result<HtmlElement, JsValue> {
        let d = &self.document;
        let mut class = "CodeMirror-line".to_string();
        if let Some((_, Some(text_class))) = &line.styles.classes {
            class.push(' ');
            class.push_str(text_class);
        }
        let pre = el(d, "pre", &class)?;
        pre.set_attribute("role", "presentation")?;
        let span = el(d, "span", "")?;
        span.set_attribute("role", "presentation")?;
        for seg in frame::line_segments(&line.text, &line.styles, tab_size) {
            let node: web_sys::Node = match seg {
                Seg::Text { text, class: None } => d.create_text_node(&text).into(),
                Seg::Text {
                    text,
                    class: Some(c),
                } => {
                    let s = el(d, "span", &c)?;
                    s.set_text_content(Some(&text));
                    s.into()
                }
                Seg::Tab { n, class } => {
                    let c = class.map_or("cm-tab".to_string(), |c| format!("{c} cm-tab"));
                    let s = el(d, "span", &c)?;
                    s.set_attribute("cm-text", "\t")?;
                    s.set_text_content(Some(&" ".repeat(n as usize)));
                    s.into()
                }
                Seg::Special { code, class } => {
                    let c = class.map_or("cm-invalidchar".to_string(), |c| {
                        format!("{c} cm-invalidchar")
                    });
                    let s = el(d, "span", &c)?;
                    s.set_attribute("title", &format!("\\u{code:x}"))?;
                    s.set_attribute("cm-text", &String::from_utf16_lossy(&[code]))?;
                    s.set_text_content(Some("\u{2022}"));
                    s.into()
                }
            };
            span.append_child(&node)?;
        }
        if line.text.is_empty() {
            // `buildLineContent`: an empty line holds a zero-width space.
            span.append_child(&d.create_text_node("\u{200b}"))?;
        }
        pre.append_child(&span)?;
        Ok(pre)
    }

    /// Write a frame into the elements.
    pub fn apply(
        &mut self,
        f: &Frame,
        tab_size: i64,
        preedit: Option<&str>,
    ) -> Result<(), JsValue> {
        self.frames += 1;
        let root_class = format!(
            "CodeMirror cm-web cm-s-default{}{}",
            if f.focused { " CodeMirror-focused" } else { "" },
            if f.wrap_width.is_some() {
                " CodeMirror-wrap"
            } else {
                ""
            },
        );
        if self.root.class_name() != root_class {
            self.root.set_class_name(&root_class);
        }
        let g = f.gutter_width;
        css(&self.sizer, "margin-left", &px(g));
        css(&self.sizer, "height", &px(f.content_height));
        css(&self.sizer, "width", &px(f.content_width.max(1.0)));
        css(&self.lines_div, "top", &px(f.padding_top));
        let lines_w = match f.wrap_width {
            Some(w) => w + 8.0,
            None => f.content_width.max(1.0),
        };
        css(&self.lines_div, "width", &px(lines_w));
        css(&self.root, "line-height", &px(f.text_height));
        let first_top = f.lines.first().map_or(0.0, |l| l.top);
        css(&self.code, "top", &px(first_top));

        // Lines: reuse a pre while its text, styles and wrap width are unchanged.
        let mut keep = HashMap::with_capacity(f.lines.len());
        let mut order = Vec::with_capacity(f.lines.len());
        for line in &f.lines {
            let reuse = match self.cache.remove(&line.id) {
                Some(c)
                    if c.text == line.text
                        && Rc::ptr_eq(&c.styles, &line.styles)
                        && c.wrap == f.wrap_width
                        && c.tab_size == tab_size =>
                {
                    Some(c)
                }
                Some(c) => {
                    c.el.remove();
                    None
                }
                None => None,
            };
            let entry = match reuse {
                Some(c) => c,
                None => CachedLine {
                    text: line.text.clone(),
                    styles: line.styles.clone(),
                    wrap: f.wrap_width,
                    tab_size,
                    el: self.build_line(line, tab_size)?,
                },
            };
            css(&entry.el, "height", &px(line.height));
            order.push(line.id);
            keep.insert(line.id, entry);
        }
        for (_, c) in self.cache.drain() {
            c.el.remove();
        }
        if order != self.order || self.code.child_element_count() as usize != order.len() {
            for id in &order {
                self.code.append_child(&keep[id].el)?;
            }
        }
        self.cache = keep;
        self.order = order;

        // Selection boxes and cursors (`drawSelectionRange`, `drawSelectionCursor`).
        self.sel_div.set_text_content(None);
        for r in &f.selections {
            let b = el(&self.document, "div", "CodeMirror-selected")?;
            css(&b, "left", &px(r.left));
            css(&b, "top", &px(r.top));
            css(&b, "width", &px(r.right - r.left));
            css(&b, "height", &px(r.bottom - r.top));
            self.sel_div.append_child(&b)?;
        }
        self.cursors_div.set_text_content(None);
        self.cursors_div.set_class_name(if f.overwrite {
            "CodeMirror-cursors CodeMirror-overwrite"
        } else {
            "CodeMirror-cursors"
        });
        for r in &f.cursors {
            let c = el(&self.document, "div", "CodeMirror-cursor")?;
            css(&c, "left", &px(r.left));
            css(&c, "top", &px(r.top));
            css(&c, "height", &px(r.bottom - r.top));
            self.cursors_div.append_child(&c)?;
        }

        // Gutter (line numbers), aligned to scrollLeft (`alignHorizontally`).
        if g > 0.0 {
            css(&self.gutter, "display", "");
            css(&self.gutter, "width", &px(g - 1.0));
            css(&self.gutter, "height", &px(f.content_height));
            css(&self.gutter, "left", &px(f.scroll_left));
            let key: Vec<(String, i64)> = f
                .lines
                .iter()
                .filter_map(|l| l.number.clone().map(|n| (n, l.top as i64)))
                .collect();
            if key != self.numbers_key {
                self.gutter.set_text_content(None);
                for (line, (n, _)) in f.lines.iter().zip(&key) {
                    let d = el(&self.document, "div", "CodeMirror-linenumber")?;
                    css(&d, "top", &px(line.top + f.padding_top));
                    d.set_text_content(Some(n));
                    self.gutter.append_child(&d)?;
                }
                self.numbers_key = key;
            }
        } else {
            css(&self.gutter, "display", "none");
        }

        // The input field and the preedit sit at the primary cursor
        // (`TextareaInput.prepareSelection`).
        let (cx, cy, ch) = f.cursors.first().map_or((0.0, 0.0, f.text_height), |c| {
            (c.left, c.top, c.bottom - c.top)
        });
        let fx = (cx + g - f.scroll_left).clamp(0.0, (f.width - 3.0).max(0.0));
        let fy = (cy + f.padding_top - f.scroll_top).clamp(0.0, (f.height - ch).max(0.0));
        css(&self.field_wrap, "left", &px(fx));
        css(&self.field_wrap, "top", &px(fy + ch));
        match preedit.filter(|p| !p.is_empty()) {
            Some(p) => {
                self.preedit.set_text_content(Some(p));
                css(&self.preedit, "display", "");
                css(&self.preedit, "left", &px(cx));
                css(&self.preedit, "top", &px(cy));
            }
            None => css(&self.preedit, "display", "none"),
        }

        // Scroll position last, after the content size (`setScrollTop`).
        let (st, sl) = (
            self.scroller.scroll_top() as f64,
            self.scroller.scroll_left() as f64,
        );
        if (st - f.scroll_top).abs() >= 1.0 {
            self.scroller.set_scroll_top(f.scroll_top.round() as i32);
        }
        if (sl - f.scroll_left).abs() >= 1.0 {
            self.scroller.set_scroll_left(f.scroll_left.round() as i32);
        }
        Ok(())
    }

    /// Event position relative to the scroller's border box (window pixels of
    /// `App`'s input, where x < gutter width is the gutter).
    fn local(&self, client_x: f64, client_y: f64) -> (f64, f64) {
        let r = self.scroller.get_bounding_client_rect();
        (client_x - r.left(), client_y - r.top())
    }

    pub fn rendered_lines(&self) -> usize {
        self.order.len()
    }
}

/// Run `f` on the editor state if it is alive and not already borrowed (a
/// reentrant event during an editor call), then the deferred DOM work.
fn with_inner(w: &Weak<RefCell<Inner>>, f: impl FnOnce(&mut Inner) -> Post) {
    let Some(rc) = w.upgrade() else { return };
    let (post, field) = {
        let Ok(mut inner) = rc.try_borrow_mut() else {
            return;
        };
        let field = inner.dom.as_ref().map(|d| d.field.clone());
        (f(&mut inner), field)
    };
    if post.focus
        && let Some(field) = field
    {
        let _ = field.focus();
    }
}

fn mods_of(e: &MouseEvent) -> cm_view::KeyEvent {
    cm_view::KeyEvent {
        shift: e.shift_key(),
        ctrl: e.ctrl_key(),
        alt: e.alt_key(),
        meta: e.meta_key(),
        ..Default::default()
    }
}

fn on_key_down(inner: &mut Inner, e: &KeyboardEvent) -> Post {
    let key = key_event_from_dom(
        e.key_code(),
        Some(&e.code()),
        [e.shift_key(), e.ctrl_key(), e.alt_key(), e.meta_key()],
        false,
    );
    let composing = e.is_composing() || e.key_code() == 229;
    if inner.session.key_down(key, composing) {
        e.prevent_default();
        inner.after_input(400);
    }
    Post::default()
}

fn on_before_input(inner: &mut Inner, e: &InputEvent) -> Post {
    let mut kind = BeforeInput::from_dom(&e.input_type(), e.data());
    if e.is_composing() {
        kind = BeforeInput::Composition;
    }
    if inner.session.before_input(kind) {
        e.prevent_default();
        inner.after_input(400);
    }
    Post::default()
}

fn on_input(inner: &mut Inner, _e: &Event) -> Post {
    if inner.session.preedit().is_some() {
        return Post::default();
    }
    if let Some(d) = inner.dom.as_ref() {
        let v = d.field.value();
        d.field.set_value("");
        if inner.session.poll(&v) {
            inner.after_input(400);
        }
    }
    Post::default()
}

fn on_comp_start(inner: &mut Inner, _e: &CompositionEvent) -> Post {
    inner.session.composition_start();
    inner.schedule_frame();
    Post::default()
}

fn on_comp_update(inner: &mut Inner, e: &CompositionEvent) -> Post {
    inner
        .session
        .composition_update(&e.data().unwrap_or_default());
    inner.schedule_frame();
    Post::default()
}

fn on_comp_end(inner: &mut Inner, e: &CompositionEvent) -> Post {
    inner.session.composition_end(&e.data().unwrap_or_default());
    if let Some(d) = inner.dom.as_ref() {
        d.field.set_value("");
    }
    inner.after_input(400);
    Post::default()
}

fn on_copy(inner: &mut Inner, e: &ClipboardEvent) -> Post {
    let Some(dt) = e.clipboard_data() else {
        return Post::default();
    };
    if let Some(t) = inner.session.copy() {
        let _ = dt.set_data("text/plain", &t);
        e.prevent_default();
    }
    Post::default()
}

fn on_cut(inner: &mut Inner, e: &ClipboardEvent) -> Post {
    let Some(dt) = e.clipboard_data() else {
        return Post::default();
    };
    if let Some(t) = inner.session.cut() {
        let _ = dt.set_data("text/plain", &t);
        e.prevent_default();
        inner.after_input(400);
    }
    Post::default()
}

fn on_paste(inner: &mut Inner, e: &ClipboardEvent) -> Post {
    let text = e
        .clipboard_data()
        .and_then(|dt| dt.get_data("text/plain").ok())
        .unwrap_or_default();
    e.prevent_default();
    if !text.is_empty() {
        inner.session.paste(&text);
        inner.after_input(400);
    }
    Post::default()
}

fn on_focus(inner: &mut Inner, _e: &Event) -> Post {
    inner.session.app.set_focused(true);
    inner.restart_blink();
    // `onFocus` adds `CodeMirror-focused` at once; draw now, not at the next frame.
    inner.render();
    Post::default()
}

fn on_blur(inner: &mut Inner, _e: &Event) -> Post {
    inner.session.app.set_focused(false);
    if let Some(d) = inner.dom.as_mut() {
        d.dragging = false;
    }
    inner.restart_blink();
    inner.render();
    Post::default()
}

fn on_mouse_down(inner: &mut Inner, e: &MouseEvent) -> Post {
    let Some(d) = inner.dom.as_mut() else {
        return Post::default();
    };
    let button = e.button();
    if !(0..=2).contains(&button) {
        return Post::default();
    }
    // A press on the native scrollbar is not editor input (`eventInWidget`/the
    // scrollbar check of `posFromMouse`).
    if e.target().as_ref() == Some(d.scroller.as_ref())
        && (e.offset_x() >= d.scroller.client_width() || e.offset_y() >= d.scroller.client_height())
    {
        return Post::default();
    }
    let (x, y) = d.local(e.client_x() as f64, e.client_y() as f64);
    if button == 0 {
        e.prevent_default();
        d.dragging = true;
    }
    inner.session.handle(Input::MouseDown {
        x,
        y,
        button: button as u8 + 1,
        mods: mods_of(e),
        time: e.time_stamp(),
    });
    inner.after_input(400);
    Post { focus: true }
}

fn on_mouse_move(inner: &mut Inner, e: &MouseEvent) -> Post {
    let Some(d) = inner.dom.as_mut() else {
        return Post::default();
    };
    if !d.dragging {
        return Post::default();
    }
    let (x, y) = d.local(e.client_x() as f64, e.client_y() as f64);
    if e.buttons() == 0 {
        // The button came up outside the window (`leftButtonSelect`: `buttons == 0`).
        d.dragging = false;
        inner.session.handle(Input::MouseUp { x, y });
    } else {
        inner.session.handle(Input::MouseMove {
            x,
            y,
            mods: mods_of(e),
        });
    }
    inner.after_input(400);
    Post::default()
}

fn on_mouse_up(inner: &mut Inner, e: &MouseEvent) -> Post {
    let Some(d) = inner.dom.as_mut() else {
        return Post::default();
    };
    if !d.dragging {
        return Post::default();
    }
    d.dragging = false;
    let (x, y) = d.local(e.client_x() as f64, e.client_y() as f64);
    inner.session.handle(Input::MouseUp { x, y });
    inner.after_input(400);
    Post::default()
}

fn on_wheel(inner: &mut Inner, e: &WheelEvent) -> Post {
    if e.ctrl_key() {
        return Post::default();
    }
    let ed = &inner.session.app.editor;
    let unit = match e.delta_mode() {
        0 => 1.0,
        1 => ed.metrics.text_height,
        _ => ed.display.height,
    };
    // The editor scrolls; the browser's default scroll would move it a second time.
    e.prevent_default();
    inner.session.handle(Input::Wheel {
        dx: e.delta_x() * unit,
        dy: e.delta_y() * unit,
    });
    inner.after_scroll();
    Post::default()
}

fn on_scroll(inner: &mut Inner, _e: &Event) -> Post {
    let Some(d) = inner.dom.as_ref() else {
        return Post::default();
    };
    let (top, left) = (
        d.scroller.scroll_top() as f64,
        d.scroller.scroll_left() as f64,
    );
    let ed = &inner.session.app.editor;
    // `updateScrollTop` ignores changes under 2px (its own echo, rounding).
    let dt = (top - ed.display.scroll_top).abs() >= 2.0;
    let dl = (left - ed.display.scroll_left).abs() >= 2.0;
    if dt || dl {
        inner
            .session
            .app
            .editor
            .scroll_to(dl.then_some(left), dt.then_some(top));
        inner.session.app.deliver_pending();
        inner.after_scroll();
    }
    Post::default()
}

fn touch_point(e: &TouchEvent, changed: bool) -> Option<(f64, f64)> {
    let list = if changed {
        e.changed_touches()
    } else {
        e.touches()
    };
    if list.length() != 1 {
        return None;
    }
    let t = list.get(0)?;
    Some((t.client_x() as f64, t.client_y() as f64))
}

fn on_touch_start(inner: &mut Inner, e: &TouchEvent) -> Post {
    if let Some(d) = inner.dom.as_mut() {
        d.touch = touch_point(e, false).map(|(x, y)| Touch {
            x,
            y,
            time: e.time_stamp(),
            moved: false,
        });
    }
    Post::default()
}

fn on_touch_move(inner: &mut Inner, _e: &TouchEvent) -> Post {
    if let Some(t) = inner.dom.as_mut().and_then(|d| d.touch.as_mut()) {
        t.moved = true;
    }
    Post::default()
}

/// A tap (not moved, under 300 ms, `onTouchStart`/`touchend` in CodeMirror.js)
/// places the cursor through the mouse path; the browser's compatibility mouse
/// events are cancelled so the tap is applied once. Moves scroll natively.
fn on_touch_end(inner: &mut Inner, e: &TouchEvent) -> Post {
    let Some(d) = inner.dom.as_mut() else {
        return Post::default();
    };
    let Some(t) = d.touch.take() else {
        return Post::default();
    };
    if t.moved || e.time_stamp() - t.time >= 300.0 {
        return Post::default();
    }
    let (cx, cy) = touch_point(e, true).unwrap_or((t.x, t.y));
    let (x, y) = d.local(cx, cy);
    e.prevent_default();
    inner.session.handle(Input::MouseDown {
        x,
        y,
        button: 1,
        mods: Default::default(),
        time: e.time_stamp(),
    });
    inner.session.handle(Input::MouseUp { x, y });
    inner.after_input(400);
    Post { focus: true }
}
