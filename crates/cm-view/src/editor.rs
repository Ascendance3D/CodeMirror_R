//! The editor: `src/edit/CodeMirror.js`, `src/edit/methods.js`, the operation and
//! scrolling parts of `src/display/*`, `src/input/indent.js`, `src/input/input.js`
//! and `src/edit/key_events.js`, over a [`cm_core::Doc`].
//!
//! There is no DOM. Measurement is the box model in [`crate::measure`]; the
//! "display" is a scroll position, a size, and the viewport (`viewFrom`/`viewTo`)
//! that decides which lines a renderer draws and which keep a saved mode state.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use cm_core::pos::{Sticky, cmp};
use cm_core::text::{Text, from_u16, is_js_space, split_lines_auto, to_u16};
use cm_core::util::misc::count_column;
use cm_core::{ChangeInfo, Doc, DocOptions, EditorSemantics, Pos, SelOptions, SelRange};
use cm_modes::{ConfigValue, Mode, ModeOptions, ModeSpec, Registry, Spec, inner_mode};

use crate::highlight::{self, HighlightState, Styles, Token};
use crate::keymap::{Binding, KeyEvent, KeyMap, KeyMapRef, KeyMaps, Lookup, key_name, lookup_key};
use crate::measure::{LineLayout, Metrics, PosInfo, Rect, coords_char_inner};
use crate::movement::{UnitH, UnitV, find_pos_h, find_pos_v};

/// A command (`CodeMirror.commands[name]`); `false` is `CodeMirror.Pass`.
pub type CommandFn = Rc<dyn Fn(&mut Editor) -> bool>;

/// Editor options (`CodeMirror.defaults`, src/edit/options.js), the ones this port
/// implements.
#[derive(Clone, Debug)]
pub struct EditorOptions {
    pub tab_size: i64,
    pub indent_unit: i64,
    pub indent_with_tabs: bool,
    pub smart_indent: bool,
    pub electric_chars: bool,
    pub line_wrapping: bool,
    pub read_only: bool,
    pub line_numbers: bool,
    pub first_line_number: i64,
    /// `keyMap`: a global key map name.
    pub key_map: String,
    /// `rtlMoveVisually` (`!windows`).
    pub rtl_move_visually: bool,
    pub line_wise_copy_cut: bool,
    pub paste_lines_per_selection: bool,
    pub cursor_scroll_margin: f64,
    pub viewport_margin: i64,
    pub drag_drop: bool,
    pub undo_depth: f64,
    pub history_event_delay: f64,
    pub max_highlight_length: usize,
    pub flatten_spans: bool,
    /// `lineSeparator`.
    pub line_separator: Option<String>,
    /// The platform is a Mac (`mac`, `flipCtrlCmd`).
    pub mac: bool,
}

impl Default for EditorOptions {
    fn default() -> Self {
        EditorOptions {
            tab_size: 4,
            indent_unit: 2,
            indent_with_tabs: false,
            smart_indent: true,
            electric_chars: true,
            line_wrapping: false,
            read_only: false,
            line_numbers: false,
            first_line_number: 1,
            key_map: "default".into(),
            rtl_move_visually: !cfg!(windows),
            line_wise_copy_cut: true,
            paste_lines_per_selection: true,
            cursor_scroll_margin: 0.0,
            viewport_margin: 10,
            drag_drop: true,
            undo_depth: 200.0,
            history_event_delay: 1250.0,
            max_highlight_length: 10000,
            flatten_spans: true,
            line_separator: None,
            mac: cfg!(target_os = "macos"),
        }
    }
}

/// Coordinate systems (`intoCoordSystem` contexts): "line" (line-local), "div"
/// (`display.lineDiv`, here: document y without the top padding) and "local"
/// (the sizer, i.e. including `paddingTop`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordSys {
    Line,
    Div,
    Local,
}

/// The parts of `cm.display` the model needs.
#[derive(Clone, Debug)]
pub struct Display {
    /// `display.wrapper.clientWidth/clientHeight`.
    pub width: f64,
    pub height: f64,
    /// `window.innerHeight` (page motion takes the smaller of this and `height`).
    pub window_height: Option<f64>,
    pub scroll_top: f64,
    pub scroll_left: f64,
    /// `display.shift`: shift is held during the current key/mouse event.
    pub shift: bool,
    pub view_from: i64,
    pub view_to: i64,
}

/// `lastCopied` (module state in src/input/input.js).
#[derive(Clone, Debug, PartialEq)]
pub struct LastCopied {
    pub line_wise: bool,
    pub text: Vec<String>,
}

thread_local! {
    static LAST_COPIED: RefCell<Option<LastCopied>> = const { RefCell::new(None) };
}

/// `setLastCopied`.
pub fn set_last_copied(v: Option<LastCopied>) {
    LAST_COPIED.with(|l| *l.borrow_mut() = v);
}
pub fn last_copied() -> Option<LastCopied> {
    LAST_COPIED.with(|l| l.borrow().clone())
}

#[derive(Default, Clone, Debug)]
struct OpState {
    depth: u32,
    scroll_to_pos: Option<(Pos, Pos, f64)>,
    scroll_top: Option<f64>,
    scroll_left: Option<f64>,
}

/// Heights of wrapped lines (`line.height` when `lineWrapping` is on).
#[derive(Default, Debug)]
pub(crate) struct HeightCache {
    wrap_width: f64,
    /// Per line; NaN = needs layout.
    heights: Vec<f64>,
    /// Prefix sums, `prefix[i]` = height of lines before i.
    prefix: Option<Vec<f64>>,
    /// Width of the widest line (`display.maxLine`), recomputed after changes.
    max_width: Option<f64>,
}

pub(crate) struct MouseState {
    pub(crate) sel: crate::mouse::SelectDrag,
}

/// The editor.
pub struct Editor {
    pub doc: Doc,
    pub options: EditorOptions,
    pub registry: Rc<Registry>,
    pub mode_spec: Spec,
    pub mode: Rc<dyn Mode>,
    pub metrics: Metrics,
    pub hs: Rc<RefCell<HighlightState>>,
    pub display: Display,
    /// `cm.state.overwrite`.
    pub overwrite: bool,
    /// `cm.state.pasteIncoming` / `cutIncoming` (timestamps).
    pub paste_incoming: f64,
    pub cut_incoming: f64,
    /// `cm.state.keySeq`.
    pub key_seq: Option<String>,
    /// `cm.state.keyMaps` (added with `addKeyMap`), searched first.
    pub key_maps: Vec<KeyMapRef>,
    pub extra_keys: Option<KeyMapRef>,
    pub keymaps: KeyMaps,
    /// `CodeMirror.commands`, including user-defined ones (`save`, ...).
    pub commands: HashMap<String, CommandFn>,
    /// `getHelper(pos, "wordChars")` per mode name (none of the ported modes
    /// register one).
    pub keyhandled: Vec<String>,
    op: OpState,
    pub(crate) heights: Rc<RefCell<HeightCache>>,
    pub(crate) mouse: Option<MouseState>,
    pub(crate) clicks: crate::mouse::ClickTracker,
    /// Gutter width in px (line numbers), recomputed by [`Editor::update_gutter`].
    pub gutter_width: f64,
}

fn sel_opts(origin: Option<&str>, bias: Option<i64>, scroll: Option<bool>) -> SelOptions {
    SelOptions {
        origin: origin.map(str::to_string),
        bias,
        scroll,
        clear_redo: None,
    }
}
/// `sel_move`.
pub fn sel_move() -> Option<SelOptions> {
    Some(sel_opts(Some("+move"), None, None))
}
/// `sel_dontScroll`.
pub fn sel_dont_scroll() -> Option<SelOptions> {
    Some(sel_opts(None, None, Some(false)))
}
/// `sel_mouse`.
pub fn sel_mouse() -> Option<SelOptions> {
    Some(sel_opts(Some("*mouse"), None, None))
}

/// `spaceStr(n)`; a fractional count gives `"undefined"` like the JS array lookup.
pub(crate) fn space_str(n: f64) -> String {
    if n.fract() != 0.0 || n.is_nan() {
        return "undefined".into();
    }
    " ".repeat(n.max(0.0) as usize)
}

impl Editor {
    /// `CodeMirror(place, {value, mode, ...})`.
    pub fn new(text: &str, mode: Spec, options: EditorOptions, metrics: Metrics) -> Editor {
        Editor::with_registry(
            text,
            mode,
            options,
            metrics,
            Rc::new(Registry::with_all_modes()),
        )
    }

    pub fn with_registry(
        text: &str,
        mode: Spec,
        options: EditorOptions,
        metrics: Metrics,
        registry: Rc<Registry>,
    ) -> Editor {
        let doc = Doc::with_options(DocOptions {
            text: text.to_string(),
            line_sep: options.line_separator.clone(),
            ..Default::default()
        });
        Editor::with_doc(doc, mode, options, metrics, registry)
    }

    /// An editor over an existing document (`CodeMirror(place, {value: doc})`).
    pub fn with_doc(
        doc: Doc,
        mode: Spec,
        options: EditorOptions,
        metrics: Metrics,
        registry: Rc<Registry>,
    ) -> Editor {
        doc.attach_editor_semantics(EditorSemantics {
            history_event_delay: options.history_event_delay,
            whole_line_update_before: true,
            selections_may_touch: false,
            undo_depth: options.undo_depth,
        });
        let mode_opts = ModeOptions {
            indent_unit: options.indent_unit,
            tab_size: options.tab_size,
            highlight_non_standard_property_keywords: None,
        };
        let m = registry.get_mode(&mode_opts, &mode);
        let first = doc.first_line();
        let mut hs = HighlightState::new(first);
        hs.tab_size = options.tab_size;
        hs.max_highlight_length = options.max_highlight_length;
        hs.flatten_spans = options.flatten_spans;
        let hs = Rc::new(RefCell::new(hs));
        let heights = Rc::new(RefCell::new(HeightCache::default()));
        {
            let hs2 = hs.clone();
            let h2 = heights.clone();
            doc.on_change(move |d, info: &ChangeInfo| {
                // makeChangeSingleDocInEditor: `retreatFrontier(doc, from.line)`.
                highlight::retreat_frontier(d, &mut hs2.borrow_mut(), info.from.line);
                let mut h = h2.borrow_mut();
                if !h.heights.is_empty() {
                    let first = d.first_line();
                    let a = (info.from.line - first).max(0) as usize;
                    let b = ((info.to.line - first + 1).max(0) as usize).min(h.heights.len());
                    if a <= b {
                        h.heights
                            .splice(a..b, std::iter::repeat_n(f64::NAN, info.text_lines));
                    } else {
                        h.heights.clear();
                    }
                }
                h.prefix = None;
                h.max_width = None;
            });
        }
        let mut ed = Editor {
            doc,
            registry,
            mode_spec: mode,
            mode: m,
            metrics,
            hs,
            display: Display {
                width: 800.0,
                height: 600.0,
                window_height: None,
                scroll_top: 0.0,
                scroll_left: 0.0,
                shift: false,
                view_from: first,
                view_to: first,
            },
            overwrite: false,
            paste_incoming: -1.0,
            cut_incoming: -1.0,
            key_seq: None,
            key_maps: Vec::new(),
            extra_keys: None,
            keymaps: KeyMaps::standard(options.mac),
            commands: crate::commands::builtin_commands(),
            keyhandled: Vec::new(),
            op: OpState::default(),
            heights,
            mouse: None,
            clicks: Default::default(),
            gutter_width: 0.0,
            options,
        };
        ed.update_gutter();
        ed.update_view();
        ed.render_view_styles();
        ed
    }

    // ------------------------------------------------------------------------------
    // Options and mode

    /// `setOption("mode", spec)`: `loadMode` + `resetModeState`.
    pub fn set_mode(&mut self, spec: Spec) {
        let mode_opts = ModeOptions {
            indent_unit: self.options.indent_unit,
            tab_size: self.options.tab_size,
            highlight_non_standard_property_keywords: None,
        };
        self.mode = self.registry.get_mode(&mode_opts, &spec);
        self.mode_spec = spec;
        highlight::reset_mode_state(&self.doc, &mut self.hs.borrow_mut());
    }
    /// `setOption("tabSize", n)`: `resetModeState`, `clearCaches`.
    pub fn set_tab_size(&mut self, n: i64) {
        self.options.tab_size = n;
        self.hs.borrow_mut().tab_size = n;
        highlight::reset_mode_state(&self.doc, &mut self.hs.borrow_mut());
        self.invalidate_heights();
    }
    /// `setOption("lineWrapping", v)`.
    pub fn set_line_wrapping(&mut self, v: bool) {
        self.options.line_wrapping = v;
        self.invalidate_heights();
        self.update_view();
    }
    /// `setSize(width, height)`.
    pub fn set_size(&mut self, width: f64, height: f64) {
        self.display.width = width;
        self.display.height = height;
        self.invalidate_heights();
        self.update_view();
    }
    /// `isReadOnly()`.
    pub fn is_read_only(&self) -> bool {
        self.options.read_only || self.doc.cant_edit()
    }
    /// `getValue()`.
    pub fn get_value(&self) -> String {
        self.doc.get_value(None)
    }
    /// `setValue(text)`.
    pub fn set_value(&mut self, text: &str) {
        self.run_op(|cm| cm.doc.set_value(text));
    }
    pub fn first_line(&self) -> i64 {
        self.doc.first_line()
    }
    pub fn last_line(&self) -> i64 {
        self.doc.last_line()
    }

    /// Gutter width: `lineNumberWidth` + gutter padding (`.CodeMirror-linenumber {
    /// padding: 0 3px 0 5px; min-width: 20px }`).
    pub fn update_gutter(&mut self) {
        if !self.options.line_numbers {
            self.gutter_width = 0.0;
            return;
        }
        let last = self.options.first_line_number + self.doc.line_count() as i64 - 1;
        let digits = last.max(1).to_string().len() as f64;
        let inner = (digits * self.metrics.char_width).max(20.0);
        self.gutter_width = (inner + 8.0 + 1.0).ceil();
    }

    // ------------------------------------------------------------------------------
    // Operations

    /// `runInOp(cm, f)` / `methodOp`: run `f` inside one operation.
    pub fn run_op<R>(&mut self, f: impl FnOnce(&mut Editor) -> R) -> R {
        let doc = self.doc.clone();
        self.op.depth += 1;
        // `extendSelection(s)` reads `doc.cm.display.shift` (src/model/selection_updates.js).
        doc.set_display_shift(self.display.shift);
        let r = doc.operation(|| f(self));
        doc.set_display_shift(self.display.shift);
        self.op.depth -= 1;
        if self.op.depth == 0 {
            self.end_operation();
        }
        r
    }

    /// `endOperation`: apply scroll requests and update the viewport.
    fn end_operation(&mut self) {
        self.take_sel_scroll();
        if let Some(st) = self.op.scroll_top.take() {
            self.set_scroll_top(st);
        }
        if let Some(sl) = self.op.scroll_left.take() {
            self.set_scroll_left(sl);
        }
        if let Some((from, to, margin)) = self.op.scroll_to_pos.take() {
            let from = self.doc.clip_pos(from);
            let to = self.doc.clip_pos(to);
            self.scroll_pos_into_view(from, to, margin);
        }
        if self.options.line_numbers {
            self.update_gutter();
        }
        self.update_view();
        self.render_view_styles();
    }

    /// The highlighting side effects of drawing the viewport (`patchDisplay` ->
    /// `buildLineContent` -> `getLineStyles(cm, line, lineNo)`): lines without
    /// current styles get them, saving `stateAfter` (for lines longer than
    /// `maxHighlightLength`, the state from *before* the line) and advancing the
    /// highlight frontier. Done at the end of every operation, like `endOperation`.
    pub fn render_view_styles(&self) {
        let (from, to) = (
            self.display.view_from,
            self.display
                .view_to
                .min(self.first_line() + self.doc.line_count() as i64),
        );
        for n in from..to {
            self.line_styles(n);
        }
    }

    /// `ensureCursorVisible(cm)`.
    pub fn ensure_cursor_visible(&mut self) {
        self.resolve_scroll_to_pos();
        let cur = self.doc.get_cursor(None);
        self.op.scroll_to_pos = Some((cur, cur, self.options.cursor_scroll_margin));
    }

    /// `addToScrollTop(cm, top)`.
    pub fn add_to_scroll_top(&mut self, top: f64) {
        self.resolve_scroll_to_pos();
        let base = self.op.scroll_top.unwrap_or(self.display.scroll_top);
        self.op.scroll_top = Some(base + top);
    }

    /// A selection change asked for `ensureCursorVisible(doc.cm)`
    /// (src/model/selection_updates.js:setSelectionNoUndo); cm-core records it as a
    /// flag, applied here in order with the other scroll requests.
    fn take_sel_scroll(&mut self) {
        if self.doc.take_scroll_request() {
            self.resolve_scroll_to_pos_raw();
            let cur = self.doc.get_cursor(None);
            self.op.scroll_to_pos = Some((cur, cur, self.options.cursor_scroll_margin));
        }
    }

    /// `resolveScrollToPos(cm)`.
    fn resolve_scroll_to_pos(&mut self) {
        self.take_sel_scroll();
        self.resolve_scroll_to_pos_raw();
    }

    fn resolve_scroll_to_pos_raw(&mut self) {
        if let Some((from, to, margin)) = self.op.scroll_to_pos.take() {
            let f = self.estimate_coords(from);
            let t = self.estimate_coords(to);
            let r = Rect {
                left: f.left.min(t.left),
                top: f.top.min(t.top) - margin,
                right: f.right.max(t.right),
                bottom: f.bottom.max(t.bottom) + margin,
            };
            let (st, sl) = self.calculate_scroll_pos(r);
            if let Some(v) = st {
                self.op.scroll_top = Some(v);
            }
            if let Some(v) = sl {
                self.op.scroll_left = Some(v);
            }
        }
    }

    /// `estimateCoords(cm, pos)` ("local").
    fn estimate_coords(&self, pos: Pos) -> Rect {
        let pos = self.doc.clip_pos(pos);
        let left = if self.options.line_wrapping {
            0.0
        } else {
            self.metrics.char_width * pos.ch as f64
        };
        let top = self.height_at_line(pos.line) + self.metrics.padding_top;
        Rect {
            left,
            right: left,
            top,
            bottom: top + self.line_height(pos.line),
        }
    }

    /// Height of the text area (`displayHeight`).
    pub fn display_height(&self) -> f64 {
        self.display.height
    }
    /// Width of the text area (`displayWidth` minus gutters).
    pub fn text_area_width(&self) -> f64 {
        (self.display.width - self.gutter_width).max(0.0)
    }

    /// `calculateScrollPos(cm, rect)`: `(scrollTop, scrollLeft)` changes, rect in
    /// "local" coordinates.
    pub fn calculate_scroll_pos(&self, mut rect: Rect) -> (Option<f64>, Option<f64>) {
        let snap_margin = self.metrics.text_height;
        if rect.top < 0.0 {
            rect.top = 0.0;
        }
        let screentop = self.op.scroll_top.unwrap_or(self.display.scroll_top);
        let screen = self.display_height();
        if rect.bottom - rect.top > screen {
            rect.bottom = rect.top + screen;
        }
        let doc_bottom = self.doc_height() + 2.0 * self.metrics.padding_top;
        let at_top = rect.top < snap_margin;
        let at_bottom = rect.bottom > doc_bottom - snap_margin;
        let mut st = None;
        if rect.top < screentop {
            st = Some(if at_top { 0.0 } else { rect.top });
        } else if rect.bottom > screentop + screen {
            let new_top = rect
                .top
                .min((if at_bottom { doc_bottom } else { rect.bottom }) - screen);
            if new_top != screentop {
                st = Some(new_top);
            }
        }
        let screenleft = self.op.scroll_left.unwrap_or(self.display.scroll_left);
        let screenw = self.text_area_width();
        let too_wide = rect.right - rect.left > screenw;
        if too_wide {
            rect.right = rect.left + screenw;
        }
        let sl = if rect.left < 10.0 {
            Some(0.0)
        } else if rect.left < screenleft {
            Some((rect.left - if too_wide { 0.0 } else { 10.0 }).max(0.0))
        } else if rect.right > screenw + screenleft - 3.0 {
            Some(rect.right + (if too_wide { 0.0 } else { 10.0 }) - screenw)
        } else {
            None
        };
        (st, sl)
    }

    /// `scrollPosIntoView(cm, pos, end, margin)`.
    pub fn scroll_pos_into_view(&mut self, pos: Pos, end: Pos, margin: f64) -> Rect {
        let (mut pos, mut end) = (pos, end);
        if !self.options.line_wrapping && pos == end {
            end = if pos.sticky == Some(Sticky::Before) {
                Pos::with_sticky(pos.line, pos.ch + 1, Some(Sticky::Before))
            } else {
                pos
            };
            if pos.ch != 0 {
                pos = Pos::with_sticky(
                    pos.line,
                    if pos.sticky == Some(Sticky::Before) {
                        pos.ch - 1
                    } else {
                        pos.ch
                    },
                    Some(Sticky::After),
                );
            }
        }
        let mut rect = Rect {
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
        };
        for _ in 0..5 {
            let mut changed = false;
            let coords = self.cursor_coords(pos, CoordSys::Local);
            let end_coords = if end == pos {
                coords
            } else {
                self.cursor_coords(end, CoordSys::Local)
            };
            rect = Rect {
                left: coords.left.min(end_coords.left),
                top: coords.top.min(end_coords.top) - margin,
                right: coords.left.max(end_coords.left),
                bottom: coords.bottom.max(end_coords.bottom) + margin,
            };
            let (st, sl) = self.calculate_scroll_pos(rect);
            let (start_top, start_left) = (self.display.scroll_top, self.display.scroll_left);
            if let Some(v) = st {
                self.set_scroll_top(v);
                if (self.display.scroll_top - start_top).abs() > 1.0 {
                    changed = true;
                }
            }
            if let Some(v) = sl {
                self.set_scroll_left(v);
                if (self.display.scroll_left - start_left).abs() > 1.0 {
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        rect
    }

    /// Largest scroll offsets.
    pub fn max_scroll(&self) -> (f64, f64) {
        let h =
            (self.doc_height() + 2.0 * self.metrics.padding_top - self.display_height()).max(0.0);
        let w = if self.options.line_wrapping {
            0.0
        } else {
            (self.max_line_width() - self.text_area_width()).max(0.0)
        };
        (h, w)
    }
    /// `setScrollTop(cm, val)`.
    pub fn set_scroll_top(&mut self, val: f64) {
        let (mh, _) = self.max_scroll();
        self.display.scroll_top = val.min(mh).max(0.0);
        self.update_view();
    }
    /// `setScrollLeft(cm, val)`.
    pub fn set_scroll_left(&mut self, val: f64) {
        let (_, mw) = self.max_scroll();
        self.display.scroll_left = val.min(mw).max(0.0);
    }
    /// `scrollTo(x, y)`.
    pub fn scroll_to(&mut self, x: Option<f64>, y: Option<f64>) {
        if let Some(y) = y {
            self.set_scroll_top(y);
        }
        if let Some(x) = x {
            self.set_scroll_left(x);
        }
    }

    /// Widest line, for the horizontal scroll extent (`display.maxLine`).
    pub fn max_line_width(&self) -> f64 {
        if let Some(w) = self.heights.borrow().max_width {
            return w;
        }
        let mut best = 0usize;
        let mut best_line = self.first_line();
        let first = self.first_line();
        let mut n = first;
        self.doc
            .each_line(first, first + self.doc.line_count() as i64, |l| {
                let len = l.text_utf16().len();
                if len > best {
                    best = len;
                    best_line = n;
                }
                n += 1;
                false
            });
        let t = self.line_text(best_line);
        let w = LineLayout::new(&self.metrics, &t, self.options.tab_size, None).width;
        self.heights.borrow_mut().max_width = Some(w);
        w
    }

    /// `visibleLines(display, doc)`: `[from, to)`.
    pub fn visible_lines(&self) -> (i64, i64) {
        let top = (self.display.scroll_top - self.metrics.padding_top)
            .floor()
            .max(0.0);
        let bottom = top + self.display.height;
        let from = self.line_at_height(top);
        let to = self.line_at_height(bottom);
        (from, to.max(from + 1))
    }

    /// Update `viewFrom`/`viewTo` (`updateDisplayIfNeeded`: visible lines plus
    /// `viewportMargin` on each side).
    pub fn update_view(&mut self) {
        let (from, to) = self.visible_lines();
        let first = self.first_line();
        let end = first + self.doc.line_count() as i64;
        let m = self.options.viewport_margin;
        let (old_from, old_to) = (self.display.view_from, self.display.view_to);
        let mut vf = (from - m).max(first);
        let mut vt = (to + m).min(end);
        // updateDisplayIfNeeded: keep up to 20 extra lines already in the view.
        if old_from < vf && vf - old_from < 20 {
            vf = old_from.max(first);
        }
        if old_to > vt && old_to - vt < 20 {
            vt = old_to.min(end);
        }
        self.display.view_from = vf;
        self.display.view_to = vt;
        let mut hs = self.hs.borrow_mut();
        hs.view_from = self.display.view_from;
        hs.view_to = self.display.view_to;
    }

    // ------------------------------------------------------------------------------
    // Geometry

    pub fn line_text(&self, n: i64) -> Text {
        self.doc.get_line_utf16(n).unwrap_or_default()
    }
    /// Content width for wrapping (`pre.CodeMirror-line` inner width).
    pub fn wrap_width(&self) -> f64 {
        (self.text_area_width() - 2.0 * self.metrics.padding_h).max(self.metrics.char_width)
    }
    /// The layout of line `n` with text `text`.
    pub fn layout_for(&self, n: i64, text: &[u16]) -> LineLayout {
        let wrap = if self.options.line_wrapping {
            Some(self.wrap_width())
        } else {
            None
        };
        let mut l = LineLayout::new(&self.metrics, text, self.options.tab_size, wrap);
        // Style spans are separate text nodes in the DOM, and `measureCharInner`
        // does not widen an extending character's range across a node boundary;
        // that only matters on lines with extending characters. The styles are the
        // line's cached ones, as `buildLineContent` -> `getLineStyles` would use (no
        // frontier update, like an external measurement).
        if text
            .iter()
            .any(|&c| cm_core::util::misc::is_extending_char(c))
            && n >= self.first_line()
            && n <= self.last_line()
        {
            let styles = {
                let mut hs = self.hs.borrow_mut();
                highlight::get_line_styles(&self.doc, &self.mode, &mut hs, n, None)
            };
            l.set_style_ends(styles.spans.iter().map(|s| s.0));
        }
        l
    }
    pub fn layout(&self, n: i64) -> (Text, LineLayout) {
        let t = self.line_text(n);
        let l = self.layout_for(n, &t);
        (t, l)
    }

    fn invalidate_heights(&self) {
        let mut h = self.heights.borrow_mut();
        h.heights.clear();
        h.prefix = None;
        h.max_width = None;
    }

    fn ensure_heights(&self) {
        let ww = self.wrap_width();
        let n = self.doc.line_count();
        {
            let h = self.heights.borrow();
            if h.prefix.is_some() && h.wrap_width == ww && h.heights.len() == n {
                return;
            }
        }
        let mut h = self.heights.borrow_mut();
        if h.wrap_width != ww || h.heights.len() != n {
            h.heights = vec![f64::NAN; n];
            h.wrap_width = ww;
        }
        let first = self.first_line();
        let mut prefix = Vec::with_capacity(n + 1);
        let mut acc = 0.0;
        prefix.push(0.0);
        for i in 0..n {
            if h.heights[i].is_nan() {
                let t = self.line_text(first + i as i64);
                h.heights[i] =
                    LineLayout::new(&self.metrics, &t, self.options.tab_size, Some(ww)).height();
            }
            acc += h.heights[i];
            prefix.push(acc);
        }
        h.prefix = Some(prefix);
    }

    /// Height of one line.
    pub fn line_height(&self, n: i64) -> f64 {
        if !self.options.line_wrapping {
            return self.metrics.text_height;
        }
        self.ensure_heights();
        let h = self.heights.borrow();
        let i = (n - self.first_line()) as usize;
        h.heights
            .get(i)
            .copied()
            .unwrap_or(self.metrics.text_height)
    }
    /// `heightAtLine(line)`: top of line `n`, document coordinates.
    pub fn height_at_line(&self, n: i64) -> f64 {
        let i = (n - self.first_line()).max(0);
        if !self.options.line_wrapping {
            return i as f64 * self.metrics.text_height;
        }
        self.ensure_heights();
        let h = self.heights.borrow();
        let p = h.prefix.as_ref().unwrap();
        p[(i as usize).min(p.len() - 1)]
    }
    /// `doc.height`.
    pub fn doc_height(&self) -> f64 {
        self.height_at_line(self.first_line() + self.doc.line_count() as i64)
    }
    /// `lineAtHeight(doc, h)`; can return `first + size` for heights past the end.
    pub fn line_at_height(&self, y: f64) -> i64 {
        let first = self.first_line();
        let size = self.doc.line_count() as i64;
        if !self.options.line_wrapping {
            let n = if y < 0.0 {
                0
            } else {
                (y / self.metrics.text_height).floor() as i64
            };
            return first + n.min(size);
        }
        self.ensure_heights();
        let h = self.heights.borrow();
        let p = h.prefix.as_ref().unwrap();
        if y < 0.0 {
            return first;
        }
        // First i with prefix[i+1] > y.
        let mut lo = 0usize;
        let mut hi = size as usize;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if p[mid + 1] > y {
                hi = mid
            } else {
                lo = mid + 1
            }
        }
        first + lo as i64
    }

    fn into_coords(&self, line: i64, mut r: Rect, sys: CoordSys) -> Rect {
        let off = match sys {
            CoordSys::Line => 0.0,
            CoordSys::Div => self.height_at_line(line),
            CoordSys::Local => self.height_at_line(line) + self.metrics.padding_top,
        };
        r.top += off;
        r.bottom += off;
        r
    }

    /// `cursorCoords(cm, pos, context)` (left-to-right lines).
    pub fn cursor_coords(&self, pos: Pos, sys: CoordSys) -> Rect {
        let (t, l) = self.layout(pos.line);
        let r = l.cursor_coords(&t, pos.ch, pos.sticky);
        self.into_coords(pos.line, r, sys)
    }
    /// `charCoords(cm, pos, context)`.
    pub fn char_coords(&self, pos: Pos, sys: CoordSys) -> Rect {
        let (t, l) = self.layout(pos.line);
        let r = l.measure_char(&t, pos.ch);
        self.into_coords(pos.line, r, sys)
    }
    /// `coordsChar(cm, x, y)` in "div" coordinates.
    pub fn coords_char(&self, x: f64, y: f64) -> PosInfo {
        let first = self.first_line();
        if y < 0.0 {
            return PosInfo {
                line: first,
                ch: 0,
                sticky: None,
                outside: -1,
                x_rel: -1.0,
            };
        }
        let line_n = self.line_at_height(y);
        let last = self.last_line();
        if line_n > last {
            return PosInfo {
                line: last,
                ch: self.line_text(last).len() as i64,
                sticky: None,
                outside: 1,
                x_rel: 1.0,
            };
        }
        let x = x.max(0.0);
        let (t, l) = self.layout(line_n);
        coords_char_inner(&l, &t, line_n, x, y - self.height_at_line(line_n))
    }
    /// `cm.coordsChar(coords, "local")`.
    pub fn coords_char_local(&self, x: f64, y: f64) -> PosInfo {
        self.coords_char(x, y - self.metrics.padding_top)
    }
    /// `defaultTextHeight()` / `defaultCharWidth()`.
    pub fn default_text_height(&self) -> f64 {
        self.metrics.text_height
    }
    pub fn default_char_width(&self) -> f64 {
        self.metrics.char_width
    }
    /// The page size `findPosV` uses.
    pub fn page_size(&self) -> f64 {
        match self.display.window_height {
            Some(w) => self.display.height.min(w),
            None => self.display.height,
        }
    }

    // ------------------------------------------------------------------------------
    // Highlighting

    /// `getLineStyles(cm, line)` for rendering; `update_frontier` as in
    /// `buildLineContent`.
    pub fn line_styles(&self, n: i64) -> Rc<Styles> {
        let mut hs = self.hs.borrow_mut();
        highlight::get_line_styles(&self.doc, &self.mode, &mut hs, n, Some(n))
    }
    /// One step of the background highlighter; returns whether work remains.
    pub fn highlight_step(&self, max_lines: usize) -> bool {
        let mut hs = self.hs.borrow_mut();
        highlight::highlight_worker(&self.doc, &self.mode, &mut hs, max_lines).1
    }
    /// `getStateAfter(line, precise)` without returning the state.
    pub fn catch_up_states(&self, line: Option<i64>, precise: bool) {
        let doc = &self.doc;
        let l = line.unwrap_or(doc.first_line() + doc.line_count() as i64 - 1);
        let l = l.clamp(doc.first_line(), doc.last_line());
        let mut hs = self.hs.borrow_mut();
        highlight::get_context_before(doc, &self.mode, &mut hs, l + 1, precise);
    }
    /// `getTokenAt(pos, precise)`.
    pub fn get_token_at(&self, pos: Pos, precise: bool) -> Token {
        let mut hs = self.hs.borrow_mut();
        highlight::take_token(&self.doc, &self.mode, &mut hs, pos, precise, false).0
    }
    /// `getLineTokens(line, precise)`.
    pub fn get_line_tokens(&self, line: i64, precise: bool) -> Vec<Token> {
        let mut hs = self.hs.borrow_mut();
        highlight::take_token(
            &self.doc,
            &self.mode,
            &mut hs,
            Pos::line_end(line),
            precise,
            true,
        )
        .1
    }
    /// `getTokenTypeAt(pos)`.
    pub fn get_token_type_at(&self, pos: Pos) -> Option<String> {
        let pos = self.doc.clip_pos(pos);
        let styles = self.line_styles(pos.line);
        let ch = pos.ch as usize;
        let spans = &styles.spans;
        let ty: Option<Rc<str>> = if ch == 0 {
            spans.first().and_then(|s| s.1.clone())
        } else {
            // Binary search over (end, style) pairs.
            let (mut before, mut after) = (0usize, spans.len());
            loop {
                if before >= after {
                    break None;
                }
                let mid = (before + after) >> 1;
                let start = if mid > 0 { spans[mid - 1].0 } else { 0 };
                if start >= ch {
                    after = mid;
                } else if spans[mid].0 < ch {
                    before = mid + 1;
                } else {
                    break spans[mid].1.clone();
                }
            }
        };
        let ty = ty?;
        match ty.find("overlay ") {
            None => Some(ty.to_string()),
            Some(0) => None,
            Some(cut) => Some(ty[..cut - 1].to_string()),
        }
    }
    /// `getModeAt(pos)`.
    pub fn get_mode_at(&self, pos: Pos) -> Rc<dyn Mode> {
        if !highlight::mode_has_inner(&self.mode) {
            return self.mode.clone();
        }
        let st = self.get_token_at(pos, false).state;
        inner_mode(self.mode.clone(), st).0
    }

    // ------------------------------------------------------------------------------
    // Selection helpers used by commands

    pub fn list_selections(&self) -> Vec<SelRange> {
        self.doc.list_selections()
    }
    /// `extendSelectionsBy(f, options)`.
    pub fn extend_selections_by(
        &mut self,
        f: impl Fn(&Editor, &SelRange) -> Pos,
        options: Option<SelOptions>,
    ) {
        self.run_op(|cm| {
            let heads: Vec<Pos> = cm.doc.list_selections().iter().map(|r| f(cm, r)).collect();
            cm.doc.extend_selections(&heads, options);
        });
    }

    // ------------------------------------------------------------------------------
    // Motion (src/edit/methods.js)

    /// `findPosH(from, amount, unit, visually)`; the bool is `hitSide`.
    pub fn find_pos_h_api(
        &self,
        from: Pos,
        amount: i64,
        unit: &str,
        visually: bool,
    ) -> (Pos, bool) {
        let mut dir = 1;
        let mut amount = amount;
        if amount < 0 {
            dir = -1;
            amount = -amount;
        }
        let unit = UnitH::parse(unit).unwrap_or(UnitH::Char);
        let mut cur = (self.doc.clip_pos(from), false);
        for _ in 0..amount {
            cur = find_pos_h(self, cur.0, dir, unit, visually);
            if cur.1 {
                break;
            }
        }
        cur
    }

    /// `moveH(dir, unit)`.
    pub fn move_h(&mut self, dir: i64, unit: UnitH) {
        let visually = self.options.rtl_move_visually;
        self.extend_selections_by(
            |cm, range| {
                if cm.display.shift || cm.doc.get_extending() || range.empty() {
                    find_pos_h(cm, range.head, dir, unit, visually).0
                } else if dir < 0 {
                    range.from()
                } else {
                    range.to()
                }
            },
            sel_move(),
        );
    }

    /// `deleteH(dir, unit)`.
    pub fn delete_h(&mut self, dir: i64, unit: UnitH) {
        self.run_op(|cm| {
            if cm.doc.something_selected() {
                cm.doc.replace_selection("", None, Some("+delete"));
            } else {
                cm.delete_near_selection(|cm, range| {
                    let (other, _) = find_pos_h(cm, range.head, dir, unit, false);
                    if dir < 0 {
                        (other, range.head)
                    } else {
                        (range.head, other)
                    }
                });
            }
        });
    }

    /// `findPosV(from, amount, unit, goalColumn)`.
    pub fn find_pos_v_api(
        &self,
        from: Pos,
        amount: i64,
        unit: UnitV,
        goal_column: Option<f64>,
    ) -> (Pos, bool) {
        let mut dir = 1;
        let mut amount = amount;
        let mut x = goal_column;
        if amount < 0 {
            dir = -1;
            amount = -amount;
        }
        let mut cur = (self.doc.clip_pos(from), false);
        for _ in 0..amount {
            let mut coords = self.cursor_coords(cur.0, CoordSys::Div);
            match x {
                None => x = Some(coords.left),
                Some(v) => coords.left = v,
            }
            let (t, hit) = find_pos_v(self, coords, dir, unit);
            cur = (t.pos(), hit);
            if hit {
                break;
            }
        }
        cur
    }

    /// `moveV(dir, unit)`.
    pub fn move_v(&mut self, dir: i64, unit: UnitV) {
        self.run_op(|cm| {
            let collapse =
                !cm.display.shift && !cm.doc.get_extending() && cm.doc.something_selected();
            let ranges = cm.doc.list_selections();
            let goal_cols = cm.doc.goal_columns();
            let primary = cm.doc.selection().primary;
            let mut goals = Vec::new();
            let mut heads = Vec::new();
            let mut scroll_add = None;
            for (i, range) in ranges.iter().enumerate() {
                if collapse {
                    heads.push(if dir < 0 { range.from() } else { range.to() });
                    continue;
                }
                let mut head_pos = cm.cursor_coords(range.head, CoordSys::Div);
                if let Some(g) = goal_cols[i] {
                    head_pos.left = g;
                }
                goals.push(head_pos.left);
                let (target, _) = find_pos_v(cm, head_pos, dir, unit);
                let pos = target.pos();
                if unit == UnitV::Page && i == primary {
                    scroll_add = Some(cm.char_coords(pos, CoordSys::Div).top - head_pos.top);
                }
                heads.push(pos);
            }
            if let Some(s) = scroll_add {
                cm.add_to_scroll_top(s);
            }
            cm.doc.extend_selections(&heads, sel_move());
            if !goals.is_empty() {
                let n = cm.doc.list_selections().len();
                for i in 0..n {
                    // `doc.sel.ranges[i].goalColumn = goals[i]` (undefined past the end).
                    cm.doc.set_goal_column(i, goals.get(i).copied());
                }
            }
        });
    }

    /// `deleteNearSelection(cm, compute)` (src/edit/deleteNearSelection.js).
    pub fn delete_near_selection(&mut self, compute: impl Fn(&Editor, &SelRange) -> (Pos, Pos)) {
        let ranges = self.doc.list_selections();
        let mut kill: Vec<(Pos, Pos)> = Vec::new();
        for r in &ranges {
            let mut to_kill = compute(self, r);
            while let Some(last) = kill.last() {
                if cmp(&to_kill.0, &last.1).is_gt() {
                    break;
                }
                let replaced = kill.pop().unwrap();
                if cmp(&replaced.0, &to_kill.0).is_lt() {
                    to_kill.0 = replaced.0;
                    break;
                }
            }
            kill.push(to_kill);
        }
        self.run_op(|cm| {
            for (from, to) in kill.iter().rev() {
                // Internal `replaceRange(doc, "", from, to, "+delete")`: no clipping,
                // swapped when reversed.
                let (f, t) = if cmp(to, from).is_lt() {
                    (*to, *from)
                } else {
                    (*from, *to)
                };
                cm.doc
                    .make_change_lines(f, t, vec![String::new()], Some("+delete"));
            }
            cm.ensure_cursor_visible();
        });
    }

    /// `findWordAt(pos)`: `(anchor, head)`.
    pub fn find_word_at(&self, pos: Pos) -> (Pos, Pos) {
        let line = self.line_text(pos.line);
        let mut start = pos.ch;
        let mut end = pos.ch;
        if !line.is_empty() {
            let len = line.len() as i64;
            if (pos.sticky == Some(Sticky::Before) || end == len) && start != 0 {
                start -= 1;
            } else {
                end += 1;
            }
            let char_at = |i: i64| -> Option<u16> {
                if i >= 0 && i < len {
                    Some(line[i as usize])
                } else {
                    None
                }
            };
            let start_char = char_at(start);
            let is_w =
                |c: Option<u16>| c.is_some_and(|c| cm_core::util::misc::is_word_char(&[c], None));
            let is_s = |c: Option<u16>| c.is_some_and(is_js_space);
            let kind = if is_w(start_char) {
                0
            } else if is_s(start_char) {
                1
            } else {
                2
            };
            let check = |c: Option<u16>| match kind {
                0 => is_w(c),
                1 => is_s(c),
                // `!/\s/.test(ch) && !isWordChar(ch)`: "" (out of range) passes.
                _ => !is_s(c) && !is_w(c),
            };
            while start > 0 && check(char_at(start - 1)) {
                start -= 1;
            }
            while end < len && check(char_at(end)) {
                end += 1;
            }
        }
        (Pos::new(pos.line, start), Pos::new(pos.line, end))
    }

    // ------------------------------------------------------------------------------
    // Indentation (src/input/indent.js)

    /// `indentLine(cm, n, how, aggressive)`. `how`: "smart", "add", "subtract",
    /// "prev", or a number of columns ([`Indent::By`]).
    pub fn indent_line_raw(&mut self, n: i64, how: Indent, aggressive: bool) -> bool {
        let mut how = how;
        let mut state = None;
        if how == Indent::Smart {
            if !self.mode.has_indent() {
                how = Indent::Prev;
            } else {
                let mut hs = self.hs.borrow_mut();
                state = Some(
                    highlight::get_context_before(&self.doc, &self.mode, &mut hs, n, false).state,
                );
            }
        }
        let tab_size = self.options.tab_size;
        let handle = self.doc.get_line_handle(n).expect("line exists");
        let text = handle.text_utf16();
        let cur_space = count_column(&text, None, tab_size, 0, 0) as f64;
        handle.set_state_after(None);
        // `line.text.match(/^\s*/)[0]`
        let ws_len = text
            .iter()
            .position(|&c| !is_js_space(c))
            .unwrap_or(text.len());
        let cur_space_string: Text = text[..ws_len].to_vec();
        let mut indentation: f64 = 0.0;
        if !aggressive && !text.iter().any(|&c| !is_js_space(c)) {
            indentation = 0.0;
            how = Indent::Not;
        } else if how == Indent::Smart {
            let r = self
                .mode
                .indent(state.as_ref().unwrap(), &text[ws_len..], &text);
            match r {
                Some(v) if v <= 150.0 || v.is_nan() => indentation = v,
                _ => {
                    // Pass, or > 150
                    if !aggressive {
                        return false;
                    }
                    how = Indent::Prev;
                }
            }
        }
        match how {
            Indent::Prev => {
                indentation = if n > self.first_line() {
                    count_column(&self.line_text(n - 1), None, tab_size, 0, 0) as f64
                } else {
                    0.0
                };
            }
            Indent::Add => indentation = cur_space + self.options.indent_unit as f64,
            Indent::Subtract => indentation = cur_space - self.options.indent_unit as f64,
            Indent::By(k) => indentation = cur_space + k as f64,
            _ => {}
        }
        // `Math.max(0, indentation)`: NaN stays NaN.
        if !indentation.is_nan() {
            indentation = indentation.max(0.0);
        }
        let mut indent_string = String::new();
        let mut pos = 0.0;
        if self.options.indent_with_tabs && !indentation.is_nan() {
            let mut i = (indentation / tab_size as f64).floor() as i64;
            while i != 0 {
                pos += tab_size as f64;
                indent_string.push('\t');
                i -= 1;
            }
        }
        if pos < indentation {
            indent_string.push_str(&space_str(indentation - pos));
        }
        if to_u16(&indent_string) != cur_space_string {
            self.doc.replace_range(
                &indent_string,
                Pos::new(n, 0),
                Some(Pos::new(n, cur_space_string.len() as i64)),
                Some("+input"),
            );
            if let Some(h) = self.doc.get_line_handle(n) {
                h.set_state_after(None);
            }
            true
        } else {
            // If the cursor was in the leading whitespace, move it to its end.
            let ranges = self.doc.list_selections();
            for (i, r) in ranges.iter().enumerate() {
                if r.head.line == n && r.head.ch < cur_space_string.len() as i64 {
                    let p = Pos::new(n, cur_space_string.len() as i64);
                    self.doc.replace_one_selection(i, p, p, None);
                    break;
                }
            }
            false
        }
    }

    /// `cm.indentLine(n, dir, aggressive)` (methodOp); `dir == None` is smart/prev.
    pub fn indent_line(&mut self, n: i64, how: Option<Indent>, aggressive: bool) {
        let how = how.unwrap_or(if self.options.smart_indent {
            Indent::Smart
        } else {
            Indent::Prev
        });
        self.run_op(|cm| {
            if n >= cm.first_line() && n <= cm.last_line() {
                cm.indent_line_raw(n, how, aggressive);
            }
        });
    }

    /// `indentSelection(how)`.
    pub fn indent_selection(&mut self, how: Indent) {
        self.run_op(|cm| {
            let ranges = cm.doc.list_selections();
            let mut end: i64 = -1;
            for (i, range) in ranges.iter().enumerate() {
                if !range.empty() {
                    let (from, to) = (range.from(), range.to());
                    let start = end.max(from.line);
                    end = cm.last_line().min(to.line - if to.ch != 0 { 0 } else { 1 }) + 1;
                    for j in start..end {
                        cm.indent_line_raw(j, how, false);
                    }
                    let new_ranges = cm.doc.list_selections();
                    if from.ch == 0
                        && ranges.len() == new_ranges.len()
                        && new_ranges[i].from().ch > 0
                    {
                        cm.doc.replace_one_selection(
                            i,
                            from,
                            new_ranges[i].to(),
                            sel_dont_scroll(),
                        );
                    }
                } else if range.head.line > end {
                    cm.indent_line_raw(range.head.line, how, true);
                    end = range.head.line;
                    if i == cm.doc.selection().primary {
                        cm.ensure_cursor_visible();
                    }
                }
            }
        });
    }

    // ------------------------------------------------------------------------------
    // Text input (src/input/input.js)

    /// `applyTextInput(cm, inserted, deleted, sel, origin)` inside an operation.
    pub fn apply_text_input(&mut self, inserted: &str, deleted: i64, origin: Option<&str>) {
        self.run_op(|cm| cm.apply_text_input_inner(inserted, deleted, origin));
    }

    fn apply_text_input_inner(&mut self, inserted: &str, deleted: i64, origin: Option<&str>) {
        self.display.shift = false;
        let sel = self.doc.list_selections();
        let recent = cm_core::clock::now() - 200.0;
        let paste = origin == Some("paste") || self.paste_incoming > recent;
        let inserted16 = to_u16(inserted);
        let text_lines: Vec<String> = split_lines_auto(&inserted16)
            .iter()
            .map(|l| from_u16(l))
            .collect();
        let mut multi_paste: Option<Vec<Vec<String>>> = None;
        let lc = last_copied();
        if paste && sel.len() > 1 {
            match &lc {
                Some(l) if l.text.join("\n") == inserted => {
                    if sel.len() % l.text.len() == 0 {
                        multi_paste =
                            Some(l.text.iter().map(|t| self.doc.split_lines(t)).collect());
                    }
                }
                _ => {
                    if text_lines.len() == sel.len() && self.options.paste_lines_per_selection {
                        multi_paste = Some(text_lines.iter().map(|l| vec![l.clone()]).collect());
                    }
                }
            }
        }
        for i in (0..sel.len()).rev() {
            let range = sel[i];
            let mut from = range.from();
            let mut to = range.to();
            if range.empty() {
                if deleted > 0 {
                    from = Pos::new(from.line, from.ch - deleted);
                } else if self.overwrite && !paste {
                    let len = self.line_text(to.line).len() as i64;
                    let last_len = to_u16(text_lines.last().unwrap()).len() as i64;
                    to = Pos::new(to.line, len.min(to.ch + last_len));
                } else if paste
                    && lc
                        .as_ref()
                        .is_some_and(|l| l.line_wise && l.text.join("\n") == text_lines.join("\n"))
                {
                    from = Pos::new(from.line, 0);
                    to = from;
                }
            }
            let text = match &multi_paste {
                Some(mp) => mp[i % mp.len()].clone(),
                None => text_lines.clone(),
            };
            let origin = match origin {
                Some(o) => o.to_string(),
                None => {
                    if paste {
                        "paste".into()
                    } else if self.cut_incoming > recent {
                        "cut".into()
                    } else {
                        "+input".into()
                    }
                }
            };
            self.doc.make_change_lines(from, to, text, Some(&origin));
        }
        if !inserted.is_empty() && !paste {
            self.trigger_electric(inserted);
        }
        self.ensure_cursor_visible();
        self.paste_incoming = -1.0;
        self.cut_incoming = -1.0;
    }

    /// `triggerElectric(cm, inserted)`.
    pub fn trigger_electric(&mut self, inserted: &str) {
        if !self.options.electric_chars || !self.options.smart_indent {
            return;
        }
        let sel = self.doc.list_selections();
        for i in (0..sel.len()).rev() {
            let range = sel[i];
            if range.head.ch > 100 || (i > 0 && sel[i - 1].head.line == range.head.line) {
                continue;
            }
            let mode = self.get_mode_at(range.head);
            if let Some(ec) = mode.electric_chars() {
                if ec.chars().any(|c| inserted.contains(c)) {
                    self.indent_line_raw(range.head.line, Indent::Smart, false);
                }
            } else {
                let line = self.line_text(range.head.line);
                let upto = &line[..(range.head.ch.max(0) as usize).min(line.len())];
                if mode.electric_input(upto) == Some(true) {
                    self.indent_line_raw(range.head.line, Indent::Smart, false);
                }
            }
        }
    }

    /// Typed text (the textarea `input` event → `poll` → `applyTextInput`).
    pub fn type_text(&mut self, text: &str) -> bool {
        if self.is_read_only() || text.is_empty() {
            return false;
        }
        self.apply_text_input(text, 0, None);
        true
    }

    /// `copyableRanges(cm)`.
    pub fn copyable_ranges(&self) -> (Vec<String>, Vec<(Pos, Pos)>) {
        let mut text = Vec::new();
        let mut ranges = Vec::new();
        for r in self.doc.list_selections() {
            let line = r.head.line;
            let (a, h) = (Pos::new(line, 0), Pos::new(line + 1, 0));
            text.push(self.doc.get_range(a, h, None));
            ranges.push((a, h));
        }
        (text, ranges)
    }

    /// The `copy` event (`prepareCopyCut`): the text for the system clipboard.
    pub fn copy(&mut self) -> Option<String> {
        if self.doc.something_selected() {
            set_last_copied(Some(LastCopied {
                line_wise: false,
                text: self.doc.get_selections(None),
            }));
            Some(self.doc.get_selection(None))
        } else if !self.options.line_wise_copy_cut {
            None
        } else {
            let (text, _) = self.copyable_ranges();
            let joined = text.join("\n");
            set_last_copied(Some(LastCopied {
                line_wise: true,
                text,
            }));
            Some(joined)
        }
    }

    /// The `cut` event: `prepareCopyCut`, then the textarea input that deletes the
    /// selection (`applyTextInput` with the "cut" origin).
    pub fn cut(&mut self) -> Option<String> {
        let out;
        if self.doc.something_selected() {
            set_last_copied(Some(LastCopied {
                line_wise: false,
                text: self.doc.get_selections(None),
            }));
            out = self.doc.get_selection(None);
        } else if !self.options.line_wise_copy_cut {
            return None;
        } else {
            let (text, ranges) = self.copyable_ranges();
            set_last_copied(Some(LastCopied {
                line_wise: true,
                text,
            }));
            let rs: Vec<(Pos, Option<Pos>)> = ranges.iter().map(|(a, h)| (*a, Some(*h))).collect();
            self.run_op(|cm| cm.doc.set_selections(&rs, None, sel_dont_scroll()));
            out = self.doc.get_selection(None);
        }
        self.cut_incoming = cm_core::clock::now();
        if !self.is_read_only() {
            self.apply_text_input("", 0, None);
        }
        Some(out)
    }

    /// `handlePaste`: paste text from the clipboard.
    pub fn paste(&mut self, text: &str) {
        if text.is_empty() || self.is_read_only() {
            return;
        }
        self.apply_text_input(text, 0, Some("paste"));
    }

    // ------------------------------------------------------------------------------
    // Commands and keys (src/edit/key_events.js)

    /// `execCommand(name)`. Unknown commands do nothing.
    pub fn exec_command(&mut self, name: &str) -> bool {
        match self.commands.get(name).cloned() {
            Some(f) => f(self),
            None => false,
        }
    }

    /// `toggleOverwrite(value)`.
    pub fn toggle_overwrite(&mut self, value: Option<bool>) {
        if let Some(v) = value {
            if v == self.overwrite {
                return;
            }
        }
        self.overwrite = !self.overwrite;
    }

    /// `doHandleBinding(cm, bound, dropShift)`.
    fn do_handle_binding(&mut self, bound: &Binding, drop_shift: bool) -> bool {
        let f: CommandFn = match bound {
            Binding::Command(name) => match self.commands.get(name) {
                Some(f) => f.clone(),
                None => return false,
            },
            Binding::Fn(f) => f.clone(),
            _ => return false,
        };
        let prev_shift = self.display.shift;
        if self.is_read_only() {
            self.doc.set_suppress_edits(true);
        }
        if drop_shift {
            self.display.shift = false;
        }
        let done = self.run_op(|cm| f(cm));
        self.display.shift = prev_shift;
        self.doc.set_suppress_edits(false);
        done
    }

    /// `lookupKeyForEditor(cm, name, handle)`.
    fn lookup_key_for_editor(
        &mut self,
        name: &str,
        handle: &mut dyn FnMut(&mut Editor, &Binding) -> bool,
    ) -> Option<Lookup> {
        let mut maps: Vec<KeyMapRef> = self.key_maps.clone();
        if let Some(e) = &self.extra_keys {
            maps.push(e.clone());
        }
        maps.push(KeyMapRef::Named(self.options.key_map.clone()));
        let keymaps = self.keymaps.clone();
        for m in maps {
            let r = lookup_with(&keymaps, name, &m, &mut |b| handle(self, b));
            if r.is_some() {
                return r;
            }
        }
        None
    }

    /// `dispatchKey(cm, name, e, handle)`.
    fn dispatch_key(
        &mut self,
        name: &str,
        handle: &mut dyn FnMut(&mut Editor, &Binding) -> bool,
    ) -> bool {
        if let Some(seq) = self.key_seq.clone() {
            if crate::keymap::is_modifier_key(name) {
                return true;
            }
            if name.ends_with('\'') {
                self.key_seq = None;
            }
            if self.dispatch_key_inner(&format!("{seq} {name}"), handle) {
                return true;
            }
            // (The JS clears the sequence after 50ms; here on the next key.)
            self.key_seq = None;
        }
        self.dispatch_key_inner(name, handle)
    }

    fn dispatch_key_inner(
        &mut self,
        name: &str,
        handle: &mut dyn FnMut(&mut Editor, &Binding) -> bool,
    ) -> bool {
        let result = self.lookup_key_for_editor(name, handle);
        if result == Some(Lookup::Multi) {
            self.key_seq = Some(name.to_string());
        }
        if result == Some(Lookup::Handled) {
            self.keyhandled.push(name.to_string());
        }
        result.is_some()
    }

    /// `handleKeyBinding(cm, e)`.
    fn handle_key_binding(&mut self, e: &KeyEvent) -> bool {
        let Some(name) = key_name(e, true, self.options.mac) else {
            return false;
        };
        if e.shift && self.key_seq.is_none() {
            self.dispatch_key(&format!("Shift-{name}"), &mut |cm, b| {
                cm.do_handle_binding(b, true)
            }) || self.dispatch_key(&name, &mut |cm, b| {
                let motion = match b {
                    Binding::Command(c) => {
                        c.len() > 2 && c.starts_with("go") && c.as_bytes()[2].is_ascii_uppercase()
                    }
                    _ => false,
                };
                if motion {
                    cm.do_handle_binding(b, false)
                } else {
                    false
                }
            })
        } else {
            self.dispatch_key(&name, &mut |cm, b| cm.do_handle_binding(b, false))
        }
    }

    /// `onKeyDown(e)`; returns whether a binding handled it.
    pub fn key_down(&mut self, e: &KeyEvent) -> bool {
        self.display.shift = e.key == "Shift" || e.shift;
        self.handle_key_binding(e)
    }

    /// `onKeyPress(e)` for a typed character: char bindings (`"'x'"`), then input.
    pub fn key_press(&mut self, ch: &str, e: &KeyEvent) -> bool {
        if e.ctrl && !e.alt || self.options.mac && e.meta {
            return false;
        }
        if ch == "\u{8}" {
            return false;
        }
        if self.dispatch_key(&format!("'{ch}'"), &mut |cm, b| {
            cm.do_handle_binding(b, true)
        }) {
            return true;
        }
        self.type_text(ch)
    }

    /// `addKeyMap(map, bottom)`.
    pub fn add_key_map(&mut self, map: KeyMap, bottom: bool) {
        let r = KeyMapRef::Map(Rc::new(map));
        if bottom {
            self.key_maps.push(r)
        } else {
            self.key_maps.insert(0, r)
        }
    }
    /// `addKeyMap(map, bottom)` with a shared map, so it can later be removed by
    /// identity (`removeKeyMap(map)`: `maps[i] == map`).
    pub fn add_key_map_rc(&mut self, map: Rc<KeyMap>, bottom: bool) {
        let r = KeyMapRef::Map(map);
        if bottom {
            self.key_maps.push(r)
        } else {
            self.key_maps.insert(0, r)
        }
    }
    /// `removeKeyMap(map)` by identity.
    pub fn remove_key_map_rc(&mut self, map: &Rc<KeyMap>) -> bool {
        let pos = self
            .key_maps
            .iter()
            .position(|m| matches!(m, KeyMapRef::Map(m) if Rc::ptr_eq(m, map)));
        match pos {
            Some(i) => {
                self.key_maps.remove(i);
                true
            }
            None => false,
        }
    }
    /// `scrollIntoView({from, to}, margin)` (src/edit/methods.js, methodOp): queue
    /// `op.scrollToPos`, resolved at the end of the operation.
    pub fn scroll_into_view(&mut self, from: Pos, to: Pos, margin: f64) {
        self.run_op(|cm| {
            cm.resolve_scroll_to_pos();
            cm.op.scroll_to_pos = Some((from, to, margin));
        });
    }
    /// `removeKeyMap(name)`.
    pub fn remove_key_map(&mut self, name: &str) -> bool {
        let pos = self.key_maps.iter().position(|m| match m {
            KeyMapRef::Named(n) => n == name,
            KeyMapRef::Map(m) => m.name.as_deref() == Some(name),
        });
        match pos {
            Some(i) => {
                self.key_maps.remove(i);
                true
            }
            None => false,
        }
    }

    /// Mode option of a resolved spec (`getOption("mode")` convenience).
    pub fn mode_name(&self) -> String {
        self.mode.name().to_string()
    }
}

/// `lookupKey` with a handler that may mutate the editor.
fn lookup_with(
    maps: &KeyMaps,
    key: &str,
    map: &KeyMapRef,
    handle: &mut dyn FnMut(&Binding) -> bool,
) -> Option<Lookup> {
    lookup_key(maps, key, map, handle)
}

/// `indentLine`'s `how` argument.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Indent {
    Smart,
    Add,
    Subtract,
    Prev,
    /// A number of columns (`typeof how == "number"`).
    By(i64),
    /// Internal: empty line, not aggressive.
    Not,
}

/// `getMode`-style resolution by file name / MIME (for opening files).
pub fn mode_for_path(path: &str) -> Spec {
    let lower = path.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    let lower = if ext == "gz" {
        lower.trim_end_matches(".gz").to_string()
    } else {
        lower
    };
    let ext = lower.rsplit('.').next().unwrap_or("");
    let mime = match ext {
        "wrl" | "vrml" | "wrz" => "model/vrml",
        "x3dv" => "model/vrml",
        "js" | "mjs" | "cjs" => "text/javascript",
        "json" => "application/json",
        "ts" => "application/typescript",
        "html" | "htm" => "text/html",
        "xml" | "x3d" | "svg" => "application/xml",
        "css" => "text/css",
        "scss" => "text/x-scss",
        "less" => "text/x-less",
        "toml" => "text/x-toml",
        _ => "text/plain",
    };
    Spec::from(mime)
}

/// A spec value helper (`{name, ...}`).
pub fn spec_named(name: &str, opts: &[(&str, ConfigValue)]) -> Spec {
    let mut s = ModeSpec::named(name);
    for (k, v) in opts {
        s.options.insert(k.to_string(), v.clone());
    }
    Spec::Obj(s)
}
