//! Generic options (src/edit/options.js, `setOption` / `getOption` of
//! src/edit/methods.js:43-52): option values by name, the per-option handlers,
//! `defineOption` for custom options, `defineInitHook`, and `phrase`.
//!
//! The editor keeps its options as typed fields ([`EditorOptions`]); this module
//! maps CM5's option names onto them. An option the port has no field for (theme,
//! inputStyle, spellcheck, scrollbarStyle, ...: DOM concerns) is not listed, and a
//! custom option ([`define_option`]) lives in a per-editor table.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use cm_modes::Spec;

use crate::editor::{Editor, EditorOptions};

/// `lineNumberFormatter`: line number to label (`integer => integer` by default).
#[derive(Clone)]
pub struct LineNumberFormatter(pub Rc<dyn Fn(i64) -> String>);

impl LineNumberFormatter {
    pub fn new(f: impl Fn(i64) -> String + 'static) -> Self {
        LineNumberFormatter(Rc::new(f))
    }
}
impl fmt::Debug for LineNumberFormatter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LineNumberFormatter(..)")
    }
}
/// Function identity, as `options[option] == value` compares functions in JS.
impl PartialEq for LineNumberFormatter {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::addr_eq(Rc::as_ptr(&self.0), Rc::as_ptr(&other.0))
    }
}

/// A function run on a new editor (`options.finishInit`, `defineInitHook`).
#[derive(Clone)]
pub struct InitHook(pub Rc<dyn Fn(&mut Editor)>);

impl InitHook {
    pub fn new(f: impl Fn(&mut Editor) + 'static) -> Self {
        InitHook(Rc::new(f))
    }
}
impl fmt::Debug for InitHook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InitHook(..)")
    }
}

/// A value of `setOption` / `getOption`.
#[derive(Clone, Debug, PartialEq)]
pub enum OptionValue {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    /// `phrases`: text to translation.
    Phrases(HashMap<String, String>),
    /// `lineNumberFormatter`.
    Formatter(LineNumberFormatter),
}

impl From<bool> for OptionValue {
    fn from(v: bool) -> Self {
        OptionValue::Bool(v)
    }
}
impl From<f64> for OptionValue {
    fn from(v: f64) -> Self {
        OptionValue::Number(v)
    }
}
impl From<i64> for OptionValue {
    fn from(v: i64) -> Self {
        OptionValue::Number(v as f64)
    }
}
impl From<&str> for OptionValue {
    fn from(v: &str) -> Self {
        OptionValue::Str(v.to_string())
    }
}

/// The old value an option handler gets: `CodeMirror.Init` when the editor is
/// being built.
#[derive(Clone, Debug, PartialEq)]
pub enum OldValue {
    Init,
    Value(OptionValue),
}

/// An option handler: `(cm, value, old)`.
pub type OptionHandler = Rc<dyn Fn(&mut Editor, &OptionValue, &OldValue)>;

struct OptionDef {
    name: String,
    default: OptionValue,
    handler: Option<OptionHandler>,
    not_on_init: bool,
}

thread_local! {
    static CUSTOM: RefCell<Vec<Rc<OptionDef>>> = const { RefCell::new(Vec::new()) };
    static INIT_HOOKS: RefCell<Vec<InitHook>> = const { RefCell::new(Vec::new()) };
}

/// Why `set_option` refused a value.
#[derive(Clone, Debug, PartialEq)]
pub enum OptionError {
    /// No such option (not built in, not defined with [`define_option`]).
    Unknown(String),
    /// The value has the wrong type for the option.
    Type(String),
    /// `defineOption` of a built-in name: the port's built-in options are fields.
    BuiltIn(String),
}

impl fmt::Display for OptionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OptionError::Unknown(n) => write!(f, "unknown option {n:?}"),
            OptionError::Type(n) => write!(f, "wrong value type for option {n:?}"),
            OptionError::BuiltIn(n) => write!(f, "{n:?} is a built-in option"),
        }
    }
}

/// The CM5 option names this port implements, in `defineOptions` order.
pub const BUILT_IN: &[&str] = &[
    "mode",
    "indentUnit",
    "indentWithTabs",
    "smartIndent",
    "tabSize",
    "lineSeparator",
    "electricChars",
    "rtlMoveVisually",
    "keyMap",
    "lineWrapping",
    "lineNumbers",
    "firstLineNumber",
    "lineNumberFormatter",
    "lineWiseCopyCut",
    "pasteLinesPerSelection",
    "selectionsMayTouch",
    "readOnly",
    "dragDrop",
    "cursorBlinkRate",
    "cursorScrollMargin",
    "flattenSpans",
    "undoDepth",
    "historyEventDelay",
    "viewportMargin",
    "maxHighlightLength",
    "autofocus",
    "phrases",
];

/// `CodeMirror.defineOption(name, default, handler, notOnInit)` for a custom
/// option: new editors get `default`, and `handler` runs when the option is set
/// (and on a new editor with [`OldValue::Init`], unless `not_on_init`).
/// Defining a name again replaces the definition. A built-in name is refused.
pub fn define_option(
    name: &str,
    default: OptionValue,
    handler: Option<OptionHandler>,
    not_on_init: bool,
) -> Result<(), OptionError> {
    if BUILT_IN.contains(&name) {
        return Err(OptionError::BuiltIn(name.to_string()));
    }
    let def = Rc::new(OptionDef {
        name: name.to_string(),
        default,
        handler,
        not_on_init,
    });
    CUSTOM.with(|c| {
        let mut c = c.borrow_mut();
        match c.iter().position(|d| d.name == name) {
            Some(i) => c[i] = def,
            None => c.push(def),
        }
    });
    Ok(())
}

fn custom(name: &str) -> Option<Rc<OptionDef>> {
    CUSTOM.with(|c| c.borrow().iter().find(|d| d.name == name).cloned())
}

/// `CodeMirror.defineInitHook(f)`: run `f` on every editor built after this, at
/// the end of its construction.
pub fn define_init_hook(f: InitHook) {
    INIT_HOOKS.with(|h| h.borrow_mut().push(f));
}

/// `CodeMirror.defaults[name]`: the default of a built-in or custom option.
pub fn default_option(name: &str) -> Option<OptionValue> {
    if BUILT_IN.contains(&name) {
        return get_field(&EditorOptions::default(), &Spec::Null, name);
    }
    custom(name).map(|d| d.default.clone())
}

fn num(v: &OptionValue, name: &str) -> Result<f64, OptionError> {
    match v {
        OptionValue::Number(n) => Ok(*n),
        _ => Err(OptionError::Type(name.to_string())),
    }
}
fn boolean(v: &OptionValue, name: &str) -> Result<bool, OptionError> {
    match v {
        OptionValue::Bool(b) => Ok(*b),
        _ => Err(OptionError::Type(name.to_string())),
    }
}

fn get_field(o: &EditorOptions, mode: &Spec, name: &str) -> Option<OptionValue> {
    use OptionValue::*;
    Some(match name {
        "mode" => match mode {
            Spec::Null => Null,
            Spec::Str(s) => Str(s.clone()),
            Spec::Obj(m) => Str(m.name.clone()),
        },
        "indentUnit" => Number(o.indent_unit as f64),
        "indentWithTabs" => Bool(o.indent_with_tabs),
        "smartIndent" => Bool(o.smart_indent),
        "tabSize" => Number(o.tab_size as f64),
        "lineSeparator" => o.line_separator.clone().map_or(Null, Str),
        "electricChars" => Bool(o.electric_chars),
        "rtlMoveVisually" => Bool(o.rtl_move_visually),
        "keyMap" => Str(o.key_map.clone()),
        "lineWrapping" => Bool(o.line_wrapping),
        "lineNumbers" => Bool(o.line_numbers),
        "firstLineNumber" => Number(o.first_line_number as f64),
        "lineNumberFormatter" => o.line_number_formatter.clone().map_or(Null, Formatter),
        "lineWiseCopyCut" => Bool(o.line_wise_copy_cut),
        "pasteLinesPerSelection" => Bool(o.paste_lines_per_selection),
        "selectionsMayTouch" => Bool(o.selections_may_touch),
        "readOnly" => Bool(o.read_only),
        "dragDrop" => Bool(o.drag_drop),
        "cursorBlinkRate" => Number(o.cursor_blink_rate),
        "cursorScrollMargin" => Number(o.cursor_scroll_margin),
        "flattenSpans" => Bool(o.flatten_spans),
        "undoDepth" => Number(o.undo_depth),
        "historyEventDelay" => Number(o.history_event_delay),
        "viewportMargin" => Number(o.viewport_margin as f64),
        "maxHighlightLength" => Number(o.max_highlight_length as f64),
        "autofocus" => Bool(o.autofocus),
        "phrases" => o.phrases.clone().map_or(Null, Phrases),
        _ => return None,
    })
}

/// Store `v` in the field of `name` (no handler).
fn set_field(o: &mut EditorOptions, name: &str, v: &OptionValue) -> Result<(), OptionError> {
    let n = || name.to_string();
    match name {
        "indentUnit" => o.indent_unit = num(v, name)? as i64,
        "indentWithTabs" => o.indent_with_tabs = boolean(v, name)?,
        "smartIndent" => o.smart_indent = boolean(v, name)?,
        "tabSize" => o.tab_size = num(v, name)? as i64,
        "lineSeparator" => {
            o.line_separator = match v {
                OptionValue::Null => None,
                OptionValue::Str(s) if s.is_empty() => None,
                OptionValue::Str(s) => Some(s.clone()),
                _ => return Err(OptionError::Type(n())),
            }
        }
        "electricChars" => o.electric_chars = boolean(v, name)?,
        "rtlMoveVisually" => o.rtl_move_visually = boolean(v, name)?,
        "keyMap" => match v {
            OptionValue::Str(s) => o.key_map = s.clone(),
            _ => return Err(OptionError::Type(n())),
        },
        "lineWrapping" => o.line_wrapping = boolean(v, name)?,
        "lineNumbers" => o.line_numbers = boolean(v, name)?,
        "firstLineNumber" => o.first_line_number = num(v, name)? as i64,
        "lineNumberFormatter" => {
            o.line_number_formatter = match v {
                OptionValue::Null => None,
                OptionValue::Formatter(f) => Some(f.clone()),
                _ => return Err(OptionError::Type(n())),
            }
        }
        "lineWiseCopyCut" => o.line_wise_copy_cut = boolean(v, name)?,
        "pasteLinesPerSelection" => o.paste_lines_per_selection = boolean(v, name)?,
        "selectionsMayTouch" => o.selections_may_touch = boolean(v, name)?,
        "readOnly" => o.read_only = boolean(v, name)?,
        "dragDrop" => o.drag_drop = boolean(v, name)?,
        "cursorBlinkRate" => o.cursor_blink_rate = num(v, name)?,
        "cursorScrollMargin" => o.cursor_scroll_margin = num(v, name)?,
        "flattenSpans" => o.flatten_spans = boolean(v, name)?,
        "undoDepth" => o.undo_depth = num(v, name)?,
        "historyEventDelay" => o.history_event_delay = num(v, name)?,
        "viewportMargin" => o.viewport_margin = num(v, name)? as i64,
        "maxHighlightLength" => o.max_highlight_length = num(v, name)?.max(0.0) as usize,
        "autofocus" => o.autofocus = boolean(v, name)?,
        "phrases" => {
            o.phrases = match v {
                OptionValue::Null => None,
                OptionValue::Phrases(p) => Some(p.clone()),
                _ => return Err(OptionError::Type(n())),
            }
        }
        _ => return Err(OptionError::Unknown(n())),
    }
    Ok(())
}

impl Editor {
    /// `getOption(name)`: `None` for a name that is neither built in nor defined
    /// (CM5: `undefined`).
    pub fn get_option(&self, name: &str) -> Option<OptionValue> {
        if let Some(v) = get_field(&self.options, &self.mode_spec, name) {
            return Some(v);
        }
        if let Some((_, v)) = self.custom_options.iter().find(|(n, _)| n == name) {
            return Some(v.clone());
        }
        custom(name).map(|d| d.default.clone())
    }

    /// `setOption(name, value)` (src/edit/methods.js:43): nothing when the value
    /// is unchanged (except for `mode`); otherwise store it, run the option's
    /// handler in an operation, then signal `optionChange` (immediate).
    pub fn set_option(
        &mut self,
        name: &str,
        value: impl Into<OptionValue>,
    ) -> Result<(), OptionError> {
        let value = value.into();
        let old = match self.get_option(name) {
            Some(o) => o,
            None => return Err(OptionError::Unknown(name.to_string())),
        };
        if old == value && name != "mode" {
            return Ok(());
        }
        if name == "mode" {
            let spec = match &value {
                OptionValue::Null => Spec::Null,
                OptionValue::Str(s) => Spec::Str(s.clone()),
                _ => return Err(OptionError::Type(name.to_string())),
            };
            // optionHandlers.mode: `doc.modeOption = val; loadMode(cm)`.
            self.set_mode(spec);
        } else if BUILT_IN.contains(&name) {
            set_field(&mut self.options, name, &value)?;
            self.run_op(|cm| cm.built_in_handler(name, &OldValue::Value(old)));
        } else {
            match self.custom_options.iter_mut().find(|(n, _)| n == name) {
                Some(e) => e.1 = value.clone(),
                None => self.custom_options.push((name.to_string(), value.clone())),
            }
            if let Some(h) = custom(name).and_then(|d| d.handler.clone()) {
                let old = OldValue::Value(old);
                self.run_op(|cm| h(cm, &value, &old));
            }
        }
        self.signal_option_change(name);
        Ok(())
    }

    /// The handlers of `optionHandlers` for the built-in options that have one.
    fn built_in_handler(&mut self, name: &str, _old: &OldValue) {
        match name {
            // loadMode: the mode reads indentUnit.
            "indentUnit" => {
                let spec = self.mode_spec.clone();
                self.set_mode(spec);
            }
            "tabSize" => {
                let n = self.options.tab_size;
                self.set_tab_size(n);
            }
            "lineWrapping" => {
                let v = self.options.line_wrapping;
                self.options.line_wrapping = !v;
                self.set_line_wrapping(v);
            }
            // updateGutters: the gutter width and a full redraw.
            "lineNumbers" | "firstLineNumber" | "lineNumberFormatter" => {
                self.update_gutter();
                self.clear_caches();
                self.invalidate_heights();
                self.force_update = true;
            }
            // resetModeState.
            "flattenSpans" | "maxHighlightLength" => {
                let spec = self.mode_spec.clone();
                self.set_mode(spec);
            }
            "undoDepth" => self.doc.set_undo_depth(self.options.undo_depth),
            // cm.refresh().
            "viewportMargin" => {
                self.clear_caches();
                self.invalidate_heights();
                self.force_update = true;
            }
            _ => {}
        }
    }

    /// `lineNumberFor(cm.options, i)` (src/line/utils_line.js:83): the label of
    /// line `i`, `lineNumberFormatter(i + firstLineNumber)`.
    pub fn line_number_for(&self, i: i64) -> String {
        let n = i + self.options.first_line_number;
        match &self.options.line_number_formatter {
            Some(f) => (f.0)(n),
            None => n.to_string(),
        }
    }

    /// `cm.phrase(text)` (src/edit/methods.js:437): the `phrases` option's
    /// translation of `text`, or `text`.
    pub fn phrase(&self, text: &str) -> String {
        self.options
            .phrases
            .as_ref()
            .and_then(|p| p.get(text).cloned())
            .unwrap_or_else(|| text.to_string())
    }

    /// The end of the constructor (src/edit/CodeMirror.js:80-90): custom option
    /// handlers with `CodeMirror.Init` (built-in ones are applied by the
    /// constructor itself), `options.finishInit`, the init hooks, and `autofocus`.
    pub(crate) fn finish_init(&mut self) {
        let defs: Vec<Rc<OptionDef>> = CUSTOM.with(|c| c.borrow().clone());
        for d in defs {
            if let (Some(h), false) = (d.handler.clone(), d.not_on_init) {
                let v = self.get_option(&d.name).unwrap_or(OptionValue::Null);
                self.run_op(|cm| h(cm, &v, &OldValue::Init));
            }
        }
        if let Some(f) = self.options.finish_init.clone() {
            (f.0)(self);
        }
        let hooks: Vec<InitHook> = INIT_HOOKS.with(|h| h.borrow().clone());
        for h in hooks {
            (h.0)(self);
        }
    }
}
