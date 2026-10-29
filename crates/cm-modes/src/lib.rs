//! Port of the CodeMirror 5 (5.65.21) mode API — `src/modes.js` — plus the
//! tokenizing loops that drive modes: `runMode` from `addon/runmode/runmode.node.js`
//! and `readToken` from `src/line/highlight.js`.
//!
//! Copyright (C) 2017 by Marijn Haverbeke <marijn@haverbeke.berlin> and others
//! (original JavaScript, MIT). Rust port, MIT.
//!
//! JS modes are objects with optional methods and an untyped state. Here a mode is
//! a [`Mode`] trait object and its state is a [`State`]: either `True` (the JS
//! `startState` fallback returns `true` when a mode has no `startState`) or a boxed
//! value of the mode's own state type.
//!
//! Mode infrastructure: [`simple`] (`addon/mode/simple.js`), [`overlay`]
//! (`addon/mode/overlay.js`), [`multiplex`] (`addon/mode/multiplex.js`), [`meta`]
//! (`mode/meta.js`) and [`helpers`] (`registerHelper` / `getHelpers` from
//! `src/edit/methods.js`). JS regexps are [`RegExp`] (`cm_core::util::regexp`).
//!
//! Modes ported so far: [`toml`] (`mode/toml/toml.js`), [`xml`] (`mode/xml/xml.js`),
//! [`javascript`] (`mode/javascript/javascript.js`, json/json-ld/typescript included),
//! [`css`] (`mode/css/css.js`, scss/less/gss included), [`htmlmixed`]
//! (`mode/htmlmixed/htmlmixed.js`), [`rust`] (`mode/rust/rust.js`, a [`simple`] mode),
//! [`clike`] (`mode/clike/clike.js`, C/C++/Java/C#/Scala/Kotlin/... included).
//! [`vrml`] is new (CM5 has no VRML mode); its JS
//! twin is `oracle/cm5-vrml-mode.js`.
//!
//! ## Copying states
//!
//! JS `copyState` without a mode `copyState` is shallow: arrays are copied, every
//! other object (linked contexts, lexical scopes) is shared with the original, and
//! later in-place mutation of a shared object shows through both states. [`State`]'s
//! `Clone` calls the state type's `Clone`; the ported modes keep object-valued fields
//! behind `Rc` (`Rc<RefCell<_>>` where the JS mutates them in place, e.g. the
//! javascript mode's `lexical.align` / `lexical.pos`), so a Rust copy shares exactly
//! what the JS copy shares.

use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;
use std::rc::Rc;

use cm_core::text::{Text, from_u16};
pub use cm_core::util::regexp::{Match, RegExp, RegExpError};
use cm_core::util::string_stream::{LineOracle, StringStream};

pub mod clike;
pub mod css;
pub mod helpers;
pub mod htmlmixed;
pub mod javascript;
pub mod js;
pub mod meta;
pub mod multiplex;
pub mod overlay;
pub mod rust;
pub mod simple;
pub mod toml;
pub mod vrml;
pub mod xml;

pub use helpers::{HelperValue, Helpers};

// --------------------------------------------------------------------------------
// State

/// A mode state value: anything `Clone + Debug + 'static`.
pub trait AnyState: Any + Debug {
    fn clone_box(&self) -> Box<dyn AnyState>;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Any + Clone + Debug> AnyState for T {
    fn clone_box(&self) -> Box<dyn AnyState> {
        Box::new(self.clone())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A mode's state.
#[derive(Debug)]
pub enum State {
    /// `startState(mode)` returns `true` for modes without `startState`.
    True,
    Value(Box<dyn AnyState>),
}

impl State {
    pub fn new<T: AnyState>(v: T) -> State {
        State::Value(Box::new(v))
    }
    /// The state as the mode's own type. Panics on a type mismatch (in JS a mode
    /// handed a foreign state would misbehave instead).
    pub fn get<T: 'static>(&self) -> &T {
        match self {
            State::Value(b) => (**b)
                .as_any()
                .downcast_ref()
                .expect("mode state of another type"),
            State::True => panic!("mode state is `true`"),
        }
    }
    pub fn get_mut<T: 'static>(&mut self) -> &mut T {
        match self {
            State::Value(b) => (**b)
                .as_any_mut()
                .downcast_mut()
                .expect("mode state of another type"),
            State::True => panic!("mode state is `true`"),
        }
    }
}

impl Clone for State {
    fn clone(&self) -> State {
        match self {
            State::True => State::True,
            State::Value(b) => State::Value((**b).clone_box()),
        }
    }
}

// --------------------------------------------------------------------------------
// Mode

/// Editor options a mode factory receives (`CodeMirror.defaults` / `cm.options`).
/// Only the fields modes commonly read are modeled.
#[derive(Clone, Debug)]
pub struct ModeOptions {
    pub indent_unit: i64,
    pub tab_size: i64,
    /// `config.highlightNonStandardPropertyKeywords` (css); `None` = undefined.
    pub highlight_non_standard_property_keywords: Option<bool>,
}

impl Default for ModeOptions {
    /// `runmode.node.js` sets `CodeMirror.defaults = { indentUnit: 2 }`.
    fn default() -> Self {
        ModeOptions {
            indent_unit: 2,
            tab_size: 8,
            highlight_non_standard_property_keywords: None,
        }
    }
}

/// A JS argument where the mode API passes "number or nothing": `startState(mode,
/// a1, a2)` arguments. `Pass` is `CodeMirror.Pass`, which htmlmixed hands to an inner
/// mode's `startState` when the html `indent` returns it (inside `<pre>`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JsArg {
    Undefined,
    Num(f64),
    Pass,
}

impl JsArg {
    /// `x != null`.
    pub fn is_some(self) -> bool {
        !matches!(self, JsArg::Undefined)
    }
    /// `x || 0` as a number. Approximation: `Pass || 0` is the Pass object in JS, and
    /// arithmetic on it gives NaN (`-`) or a string (`+`); here it is NaN.
    pub fn or_zero(self) -> f64 {
        match self {
            JsArg::Undefined => 0.0,
            JsArg::Num(x) if x == 0.0 || x.is_nan() => 0.0,
            JsArg::Num(x) => x,
            JsArg::Pass => f64::NAN,
        }
    }
}

impl From<Option<f64>> for JsArg {
    /// An `indent()` result: `None` is `CodeMirror.Pass`.
    fn from(v: Option<f64>) -> JsArg {
        match v {
            Some(x) => JsArg::Num(x),
            None => JsArg::Pass,
        }
    }
}

/// A configuration value inside a mode spec (`{name: "...", someOption: value}`).
#[derive(Clone)]
pub enum ConfigValue {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    /// Any other JS value (an object, a regexp, an array): the mode that reads the
    /// option downcasts it to the Rust type it documents.
    Obj(Rc<dyn Any>),
}

impl ConfigValue {
    /// JS truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            ConfigValue::Null => false,
            ConfigValue::Bool(b) => *b,
            ConfigValue::Num(n) => *n != 0.0 && !n.is_nan(),
            ConfigValue::Str(s) => !s.is_empty(),
            ConfigValue::Obj(_) => true,
        }
    }
    pub fn as_num(&self) -> Option<f64> {
        if let ConfigValue::Num(n) = self {
            Some(*n)
        } else {
            None
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let ConfigValue::Str(s) = self {
            Some(s)
        } else {
            None
        }
    }
    pub fn as_obj<T: 'static>(&self) -> Option<&T> {
        if let ConfigValue::Obj(o) = self {
            o.downcast_ref()
        } else {
            None
        }
    }
}

impl Debug for ConfigValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigValue::Null => write!(f, "Null"),
            ConfigValue::Bool(b) => write!(f, "Bool({b})"),
            ConfigValue::Num(n) => write!(f, "Num({n})"),
            ConfigValue::Str(s) => write!(f, "Str({s:?})"),
            ConfigValue::Obj(_) => write!(f, "Obj(..)"),
        }
    }
}

impl PartialEq for ConfigValue {
    fn eq(&self, o: &ConfigValue) -> bool {
        match (self, o) {
            (ConfigValue::Null, ConfigValue::Null) => true,
            (ConfigValue::Bool(a), ConfigValue::Bool(b)) => a == b,
            (ConfigValue::Num(a), ConfigValue::Num(b)) => a == b,
            (ConfigValue::Str(a), ConfigValue::Str(b)) => a == b,
            (ConfigValue::Obj(a), ConfigValue::Obj(b)) => {
                std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
            }
            _ => false,
        }
    }
}

/// A resolved mode spec: `{name, ...options}`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModeSpec {
    pub name: String,
    pub helper_type: Option<String>,
    pub options: HashMap<String, ConfigValue>,
    /// `spec.modeProps`: copied onto the mode object by `getMode`.
    pub mode_props: Option<HashMap<String, ConfigValue>>,
}

impl ModeSpec {
    pub fn named(name: &str) -> ModeSpec {
        ModeSpec {
            name: name.to_string(),
            ..Default::default()
        }
    }
    /// Builder: `{..., key: value}`.
    pub fn with(mut self, key: &str, v: ConfigValue) -> ModeSpec {
        self.options.insert(key.to_string(), v);
        self
    }
    /// `spec[key]`; `None` = undefined.
    pub fn get(&self, key: &str) -> Option<&ConfigValue> {
        self.options.get(key)
    }
    /// `!!spec[key]`.
    pub fn truthy(&self, key: &str) -> bool {
        self.get(key).is_some_and(ConfigValue::truthy)
    }
}

/// What can be passed where the JS accepts a mode spec: a MIME type or name, or a
/// spec object. (`null`/`undefined` is `Spec::Null`.)
#[derive(Clone, Debug, PartialEq)]
pub enum Spec {
    Null,
    Str(String),
    Obj(ModeSpec),
}

impl From<&str> for Spec {
    fn from(s: &str) -> Spec {
        Spec::Str(s.to_string())
    }
}

/// The JS mode object. Every method but `token` is optional in JS; the defaults
/// here stand for "property absent".
pub trait Mode {
    /// `mode.name`, set by `getMode`.
    fn name(&self) -> &str;
    /// `mode.startState(a1, a2)`; `None` = no `startState`.
    fn start_state(&self, _a1: JsArg, _a2: JsArg) -> Option<State> {
        None
    }
    /// `mode.token(stream, state)`: the style, `None` for null/undefined.
    fn token(&self, stream: &mut StringStream<'_>, state: &mut State) -> Option<String>;
    /// `mode.blankLine(state)`: its return value, a style that `highlight.js` scans
    /// for `line-` classes (`None` for null/undefined). Whether the method exists is
    /// [`Mode::has_blank_line`].
    fn blank_line(&self, _state: &mut State) -> Option<String> {
        None
    }
    fn has_blank_line(&self) -> bool {
        false
    }
    /// `mode.copyState`; `None` = absent, so [`copy_state`] copies generically.
    fn copy_state(&self, _state: &State) -> Option<State> {
        None
    }
    /// Whether `mode.innerMode` exists (`if (!mode.innerMode)` in `getModeAt`,
    /// `findStartLine`, `callBlankLine`).
    fn has_inner_mode(&self) -> bool {
        false
    }
    /// `mode.innerMode(state)`; `None` = absent or returned nothing. The returned
    /// state is a copy; [`Mode::inner_mode_mut`] reaches the inner state itself.
    fn inner_mode(&self, _state: &State) -> Option<(Rc<dyn Mode>, State)> {
        None
    }
    /// `mode.innerMode(state)` for callers that change the inner state (JS returns
    /// the inner state object, so `callBlankLine` updates it in place): calls `f`
    /// with the inner mode and the inner state inside `state`. Returns false, without
    /// calling `f`, when there is no inner mode. Composite modes implement both
    /// methods.
    fn inner_mode_mut(
        &self,
        _state: &mut State,
        _f: &mut dyn FnMut(&Rc<dyn Mode>, &mut State),
    ) -> bool {
        false
    }
    /// `state.combineTokens` of this mode's state, which `addon/mode/overlay.js` reads
    /// from the overlay state: `None` = null/undefined.
    fn combine_tokens(&self, _state: &State) -> Option<bool> {
        None
    }
    /// `mode.indent(state, textAfter, line)`; `None` = absent or `CodeMirror.Pass`.
    /// A number, not an integer: JS arithmetic on odd states can give NaN.
    fn indent(&self, _state: &State, _text_after: &[u16], _line: &[u16]) -> Option<f64> {
        None
    }
    /// Whether `mode.indent` exists (`if (mode.indent)`).
    fn has_indent(&self) -> bool {
        false
    }
    /// `mode.electricInput.test(lineUpToCursor)`; `None` = no `electricInput`.
    fn electric_input(&self, _text: &[u16]) -> Option<bool> {
        None
    }
    /// Any other mode-object property (`configuration`, `fold`, `closeBrackets`,
    /// `blockCommentContinue`, `jsonMode`, ...), including those `extendMode` and
    /// `modeProps` add. `None` = undefined.
    fn prop(&self, _name: &str) -> Option<ConfigValue> {
        None
    }
    fn electric_chars(&self) -> Option<&str> {
        None
    }
    fn line_comment(&self) -> Option<&str> {
        None
    }
    fn block_comment_start(&self) -> Option<&str> {
        None
    }
    fn block_comment_end(&self) -> Option<&str> {
        None
    }
    fn helper_type(&self) -> Option<&str> {
        None
    }
}

/// A `defineMode` factory: `function(config, modeConfig) -> mode`. The registry is
/// passed in because JS factories call the global `CodeMirror.getMode` /
/// `resolveMode` (css, htmlmixed).
pub type ModeFactory = Rc<dyn Fn(&Registry, &ModeOptions, &ModeSpec) -> Rc<dyn Mode>>;

/// A `defineMIME` target: a name, or a spec object.
#[derive(Clone, Debug, PartialEq)]
pub enum MimeTarget {
    Name(String),
    Spec(ModeSpec),
}

/// The `modes` / `mimeModes` tables. JS keeps them global; here they are a value.
#[derive(Clone)]
pub struct Registry {
    modes: HashMap<String, ModeFactory>,
    mime_modes: HashMap<String, MimeTarget>,
    mode_extensions: HashMap<String, HashMap<String, ConfigValue>>,
    /// `mode.dependencies`: `defineMode`'s extra arguments.
    dependencies: HashMap<String, Vec<String>>,
    /// `registerHelper` / `registerGlobalHelper` tables.
    pub helpers: Helpers,
}

impl Default for Registry {
    fn default() -> Self {
        Registry::new()
    }
}

/// The `null` mode: `{token: stream => stream.skipToEnd()}`.
struct NullMode {
    name: String,
}

impl Mode for NullMode {
    fn name(&self) -> &str {
        &self.name
    }
    fn token(&self, stream: &mut StringStream<'_>, _state: &mut State) -> Option<String> {
        stream.skip_to_end();
        None
    }
}

/// Wraps a mode to override `name` / `helperType` as `getMode` does.
struct Named {
    inner: Rc<dyn Mode>,
    name: String,
    helper_type: Option<String>,
    extensions: HashMap<String, ConfigValue>,
    mode_props: HashMap<String, ConfigValue>,
}

impl Mode for Named {
    fn name(&self) -> &str {
        &self.name
    }
    fn start_state(&self, a1: JsArg, a2: JsArg) -> Option<State> {
        if let Some(ConfigValue::Obj(f)) = self.extensions.get("startState") {
            if let Some(func) =
                f.downcast_ref::<std::rc::Rc<dyn Fn(JsArg, JsArg) -> Option<State>>>()
            {
                return func(a1, a2);
            }
        }
        self.inner.start_state(a1, a2)
    }
    fn token(&self, stream: &mut StringStream<'_>, state: &mut State) -> Option<String> {
        if let Some(ConfigValue::Obj(f)) = self.extensions.get("token") {
            if let Some(func) = f.downcast_ref::<std::rc::Rc<dyn Fn(&mut StringStream<'_>, &mut State) -> Option<String>>>() {
                return func(stream, state);
            }
        }
        self.inner.token(stream, state)
    }
    fn blank_line(&self, state: &mut State) -> Option<String> {
        if let Some(ConfigValue::Obj(f)) = self.extensions.get("blankLine") {
            if let Some(func) =
                f.downcast_ref::<std::rc::Rc<dyn Fn(&mut State) -> Option<String>>>()
            {
                return func(state);
            }
        }
        self.inner.blank_line(state)
    }
    fn has_blank_line(&self) -> bool {
        matches!(
            self.extensions.get("blankLine"),
            Some(ConfigValue::Obj(f)) if f.downcast_ref::<std::rc::Rc<dyn Fn(&mut State) -> Option<String>>>().is_some()
        ) || self.inner.has_blank_line()
    }
    fn copy_state(&self, state: &State) -> Option<State> {
        if let Some(ConfigValue::Obj(f)) = self.extensions.get("copyState") {
            if let Some(func) = f.downcast_ref::<std::rc::Rc<dyn Fn(&State) -> Option<State>>>() {
                return func(state);
            }
        }
        self.inner.copy_state(state)
    }
    fn inner_mode(&self, state: &State) -> Option<(std::rc::Rc<dyn Mode>, State)> {
        if let Some(ConfigValue::Obj(f)) = self.extensions.get("innerMode") {
            if let Some(func) = f.downcast_ref::<std::rc::Rc<dyn Fn(&State) -> Option<(std::rc::Rc<dyn Mode>, State)>>>() {
                return func(state);
            }
        }
        self.inner.inner_mode(state)
    }
    fn has_inner_mode(&self) -> bool {
        matches!(
            self.extensions.get("innerMode"),
            Some(ConfigValue::Obj(f)) if f.downcast_ref::<std::rc::Rc<dyn Fn(&State) -> Option<(std::rc::Rc<dyn Mode>, State)>>>().is_some()
        ) || self.inner.has_inner_mode()
    }
    fn inner_mode_mut(
        &self,
        state: &mut State,
        f: &mut dyn FnMut(&Rc<dyn Mode>, &mut State),
    ) -> bool {
        self.inner.inner_mode_mut(state, f)
    }
    fn combine_tokens(&self, state: &State) -> Option<bool> {
        if let Some(ConfigValue::Obj(f)) = self.extensions.get("combineTokens") {
            if let Some(func) = f.downcast_ref::<std::rc::Rc<dyn Fn(&State) -> Option<bool>>>() {
                return func(state);
            }
        }
        self.inner.combine_tokens(state)
    }
    fn indent(&self, state: &State, text_after: &[u16], line: &[u16]) -> Option<f64> {
        if let Some(ConfigValue::Obj(f)) = self.extensions.get("indent") {
            if let Some(func) =
                f.downcast_ref::<std::rc::Rc<dyn Fn(&State, &[u16], &[u16]) -> Option<f64>>>()
            {
                return func(state, text_after, line);
            }
        }
        self.inner.indent(state, text_after, line)
    }
    fn has_indent(&self) -> bool {
        matches!(
            self.extensions.get("indent"),
            Some(ConfigValue::Obj(f)) if f.downcast_ref::<std::rc::Rc<dyn Fn(&State, &[u16], &[u16]) -> Option<f64>>>().is_some()
        ) || self.inner.has_indent()
    }
    fn electric_input(&self, text: &[u16]) -> Option<bool> {
        if let Some(ConfigValue::Obj(f)) = self.extensions.get("electricInput") {
            if let Some(func) = f.downcast_ref::<std::rc::Rc<dyn Fn(&[u16]) -> Option<bool>>>() {
                return func(text);
            }
        }
        self.inner.electric_input(text)
    }
    /// `getMode` copies `modeExtensions[name]`, then `spec.modeProps`, onto the mode
    /// object: later sources win over the mode's own properties. An extension that
    /// replaces an own property keeps the old value as `"_" + prop`.
    fn prop(&self, name: &str) -> Option<ConfigValue> {
        if let Some(v) = self.mode_props.get(name) {
            return Some(v.clone());
        }
        if let Some(v) = self.extensions.get(name) {
            return Some(v.clone());
        }
        if let Some(own) = name.strip_prefix('_')
            && self.extensions.contains_key(own)
            && let Some(v) = self.inner.prop(own)
        {
            return Some(v);
        }
        self.inner.prop(name)
    }
    fn electric_chars(&self) -> Option<&str> {
        self.str_override("electricChars")
            .unwrap_or_else(|| self.inner.electric_chars())
    }
    fn line_comment(&self) -> Option<&str> {
        self.str_override("lineComment")
            .unwrap_or_else(|| self.inner.line_comment())
    }
    fn block_comment_start(&self) -> Option<&str> {
        self.str_override("blockCommentStart")
            .unwrap_or_else(|| self.inner.block_comment_start())
    }
    fn block_comment_end(&self) -> Option<&str> {
        self.str_override("blockCommentEnd")
            .unwrap_or_else(|| self.inner.block_comment_end())
    }
    fn helper_type(&self) -> Option<&str> {
        self.helper_type
            .as_deref()
            .or_else(|| self.inner.helper_type())
    }
}

impl Named {
    /// A string-valued property set by `modeProps` or an extension: `Some(value)`
    /// when one of them sets it (a non-string value reads as absent), `None` when
    /// neither does.
    fn str_override(&self, name: &str) -> Option<Option<&str>> {
        self.mode_props
            .get(name)
            .or_else(|| self.extensions.get(name))
            .map(ConfigValue::as_str)
    }
}

/// `/^[\w\-]+\/[\w\-]+\+(xml|json)$/` with the given suffix.
fn is_plus_type(s: &str, suffix: &str) -> bool {
    let Some(body) = s.strip_suffix(suffix) else {
        return false;
    };
    let word = |p: &str| {
        !p.is_empty()
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    match body.split_once('/') {
        Some((a, b)) => word(a) && word(b),
        None => false,
    }
}

impl Registry {
    /// A registry with what `runmode.node.js` (and `codemirror.js`) define up front:
    /// the `null` mode and `text/plain`.
    pub fn new() -> Registry {
        let mut r = Registry {
            modes: HashMap::new(),
            mime_modes: HashMap::new(),
            mode_extensions: HashMap::new(),
            dependencies: HashMap::new(),
            helpers: Helpers::default(),
        };
        r.define_mode(
            "null",
            Rc::new(|_, _, spec| {
                Rc::new(NullMode {
                    name: spec.name.clone(),
                }) as Rc<dyn Mode>
            }),
        );
        r.define_mime("text/plain", MimeTarget::Name("null".into()));
        r
    }
    /// A registry with every mode this crate ports.
    /// Registration order follows the CM5 test page (`test/index.html`): `xml` defines
    /// `text/html` only while it is undefined, and `htmlmixed` then redefines it.
    pub fn with_all_modes() -> Registry {
        let mut r = Registry::new();
        toml::register(&mut r);
        xml::register(&mut r);
        javascript::register(&mut r);
        css::register(&mut r);
        htmlmixed::register(&mut r);
        clike::register(&mut r);
        rust::register(&mut r);
        vrml::register(&mut r);
        r
    }
    /// `CodeMirror.extendMode(mode, properties)`.
    pub fn extend_mode(&mut self, mode: &str, properties: HashMap<String, ConfigValue>) {
        let exts = self.mode_extensions.entry(mode.to_string()).or_default();
        for (k, v) in properties {
            exts.insert(k, v);
        }
    }
    pub fn has_mime(&self, mime: &str) -> bool {
        self.mime_modes.contains_key(mime)
    }
    /// `defineMode(name, factory)`. Redefining a name replaces the factory, as the
    /// JS assignment does; the dependencies of the old definition are dropped.
    pub fn define_mode(&mut self, name: &str, factory: ModeFactory) {
        self.modes.insert(name.to_string(), factory);
        self.dependencies.remove(name);
    }
    /// `defineMode(name, factory, dep1, dep2, ...)`: the extra arguments become
    /// `mode.dependencies` (read by the legacy `loadmode.js`).
    pub fn define_mode_with_dependencies(
        &mut self,
        name: &str,
        factory: ModeFactory,
        dependencies: &[&str],
    ) {
        self.define_mode(name, factory);
        if !dependencies.is_empty() {
            self.dependencies.insert(
                name.to_string(),
                dependencies.iter().map(|d| d.to_string()).collect(),
            );
        }
    }
    /// `CodeMirror.modes[name].dependencies`.
    pub fn dependencies(&self, name: &str) -> Option<&[String]> {
        self.dependencies.get(name).map(Vec::as_slice)
    }
    /// `defineMIME(mime, spec)`.
    pub fn define_mime(&mut self, mime: &str, spec: MimeTarget) {
        self.mime_modes.insert(mime.to_string(), spec);
    }
    pub fn has_mode(&self, name: &str) -> bool {
        self.modes.contains_key(name)
    }
    /// The keys of `CodeMirror.modes`, sorted.
    pub fn mode_names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.modes.keys().map(String::as_str).collect();
        v.sort_unstable();
        v
    }
    /// `CodeMirror.mimeModes[mime]`.
    pub fn mime_target(&self, mime: &str) -> Option<&MimeTarget> {
        self.mime_modes.get(mime)
    }
    /// The keys of `CodeMirror.mimeModes`, sorted.
    pub fn mime_types(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.mime_modes.keys().map(String::as_str).collect();
        v.sort_unstable();
        v
    }
    /// `CodeMirror.modeExtensions[mode]`.
    pub fn mode_extensions(&self, mode: &str) -> Option<&HashMap<String, ConfigValue>> {
        self.mode_extensions.get(mode)
    }

    /// `resolveMode(spec)`.
    pub fn resolve_mode(&self, spec: &Spec) -> ModeSpec {
        match spec {
            Spec::Str(s) if self.mime_modes.contains_key(s) => match &self.mime_modes[s] {
                // `spec = mimeModes[spec]`, then a string becomes `{name}`.
                MimeTarget::Name(n) => ModeSpec::named(n),
                MimeTarget::Spec(o) => o.clone(),
            },
            Spec::Obj(o) if self.mime_modes.contains_key(&o.name) => {
                // `createObj(found, spec)`: spec's own properties over found's.
                let found = match &self.mime_modes[&o.name] {
                    MimeTarget::Name(n) => ModeSpec::named(n),
                    MimeTarget::Spec(f) => f.clone(),
                };
                let mut out = found.clone();
                for (k, v) in &o.options {
                    out.options.insert(k.clone(), v.clone());
                }
                if o.helper_type.is_some() {
                    out.helper_type = o.helper_type.clone();
                }
                if o.mode_props.is_some() {
                    out.mode_props = o.mode_props.clone();
                }
                out.name = found.name;
                out
            }
            Spec::Str(s) if is_plus_type(s, "+xml") => {
                self.resolve_mode(&Spec::from("application/xml"))
            }
            Spec::Str(s) if is_plus_type(s, "+json") => {
                self.resolve_mode(&Spec::from("application/json"))
            }
            Spec::Str(s) => ModeSpec::named(s),
            Spec::Obj(o) => o.clone(),
            Spec::Null => ModeSpec::named("null"),
        }
    }

    /// `getMode(options, spec)`: unknown modes fall back to `text/plain`.
    pub fn get_mode(&self, options: &ModeOptions, spec: &Spec) -> Rc<dyn Mode> {
        let spec = self.resolve_mode(spec);
        let Some(factory) = self.modes.get(&spec.name) else {
            return self.get_mode(options, &Spec::from("text/plain"));
        };
        let mode = factory(self, options, &spec);
        // `modeObj.name = spec.name; if (spec.helperType) modeObj.helperType = ...`
        // Extensions are read at getMode time, like the JS copy loop.
        Rc::new(Named {
            inner: mode,
            name: spec.name.clone(),
            helper_type: spec.helper_type.clone(),
            extensions: self
                .mode_extensions
                .get(&spec.name)
                .cloned()
                .unwrap_or_default(),
            mode_props: spec.mode_props.clone().unwrap_or_default(),
        })
    }
}

/// `copyState(mode, state)`. The generic JS copy is shallow apart from arrays;
/// `State` values are cloned, which equals that for the plain-data states of the
/// ported modes.
pub fn copy_state(mode: &dyn Mode, state: &State) -> State {
    if let State::True = state {
        return State::True;
    }
    mode.copy_state(state).unwrap_or_else(|| state.clone())
}

/// `innerMode(mode, state)` for callers that change the inner state: calls `f`
/// with the innermost mode and its state inside `state` (the outer mode and `state`
/// itself when there is no inner mode), following the same loop as [`inner_mode`].
pub fn with_inner_mode_mut<R>(
    mode: &Rc<dyn Mode>,
    state: &mut State,
    f: impl FnOnce(&Rc<dyn Mode>, &mut State) -> R,
) -> R {
    let mut f = Some(f);
    let mut out = None;
    walk_inner_mut(mode, state, &mut |m, s| {
        if let Some(f) = f.take() {
            out = Some(f(m, s));
        }
    });
    out.expect("with_inner_mode_mut calls f once")
}

fn walk_inner_mut(
    mode: &Rc<dyn Mode>,
    state: &mut State,
    f: &mut dyn FnMut(&Rc<dyn Mode>, &mut State),
) {
    let mut reached = false;
    let had = mode.inner_mode_mut(state, &mut |m, s| {
        reached = true;
        if Rc::ptr_eq(m, mode) {
            // `info.mode == mode`: the loop stops with this info.
            f(m, s);
        } else {
            walk_inner_mut(m, s, f);
        }
    });
    if !had && !reached {
        f(mode, state);
    }
}

/// `innerMode(mode, state)`.
pub fn inner_mode(mode: Rc<dyn Mode>, state: State) -> (Rc<dyn Mode>, State) {
    let mut cur = (mode, state);
    let mut info = None;
    loop {
        let next = cur.0.inner_mode(&cur.1);
        match next {
            Some((m, s)) if !Rc::ptr_eq(&m, &cur.0) => {
                info = Some((m.clone(), s.clone()));
                cur = (m, s);
            }
            _ => break,
        }
    }
    info.unwrap_or(cur)
}

/// `startState(mode, a1, a2)`.
pub fn start_state(mode: &dyn Mode, a1: JsArg, a2: JsArg) -> State {
    mode.start_state(a1, a2).unwrap_or(State::True)
}

/// Raised where the JS throws `"Mode " + name + " failed to advance stream."`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedToAdvance(pub String);

impl std::fmt::Display for FailedToAdvance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Mode {} failed to advance stream.", self.0)
    }
}

/// `readToken(mode, stream, state, inner)` from `src/line/highlight.js`: up to ten
/// tries for the mode to consume something.
pub fn read_token(
    mode: &Rc<dyn Mode>,
    stream: &mut StringStream<'_>,
    state: &mut State,
    mut inner: Option<&mut Option<Rc<dyn Mode>>>,
) -> Result<Option<String>, FailedToAdvance> {
    for _ in 0..10 {
        if let Some(slot) = inner.as_deref_mut() {
            *slot = Some(inner_mode(mode.clone(), state.clone()).0);
        }
        let style = mode.token(stream, state);
        if stream.pos > stream.start {
            return Ok(style);
        }
    }
    Err(FailedToAdvance(mode.name().to_string()))
}

// --------------------------------------------------------------------------------
// runMode

/// `CodeMirror.splitLines` in `runmode.node.js`: `string.split(/\r?\n|\r/)`.
pub fn split_lines_runmode(s: &[u16]) -> Vec<Text> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            0x0d if s.get(i + 1) == Some(&0x0a) => {
                out.push(std::mem::take(&mut cur));
                i += 2;
            }
            0x0d | 0x0a => {
                out.push(std::mem::take(&mut cur));
                i += 1;
            }
            c => {
                cur.push(c);
                i += 1;
            }
        }
    }
    out.push(cur);
    out
}

/// One `callback(text, style, lineNo, start, state, mode)` call of `runMode`.
/// Line breaks arrive as `callback("\n")`: `line`/`start` are `None` then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunToken {
    pub text: Text,
    pub style: Option<String>,
    pub line: Option<usize>,
    pub start: Option<usize>,
}

/// The `lineOracle` `runMode` gives its streams.
pub struct LinesOracle<'a> {
    pub lines: &'a [Text],
    pub i: usize,
}

impl LineOracle for LinesOracle<'_> {
    /// `lookAhead: n => lines[i + n]`.
    fn look_ahead(&self, n: i64) -> Option<Text> {
        let k = self.i as i64 + n;
        if k < 0 {
            None
        } else {
            self.lines.get(k as usize).cloned()
        }
    }
    /// `baseToken: function() {}`.
    fn base_token(&self, _pos: usize) -> Option<(Option<String>, i64)> {
        None
    }
}

/// `CodeMirror.runMode(string, modespec, callback, options)` from
/// `addon/runmode/runmode.node.js` (callback form). Like the JS there is no guard
/// against a mode that never advances: such a mode loops forever.
///
/// The JS passes `null` as the stream's tab size, so streams always use 8 —
/// `options.tabSize` only affects the DOM output form, which is not ported.
pub fn run_mode<F: FnMut(&RunToken)>(
    registry: &Registry,
    string: &str,
    spec: &Spec,
    callback: F,
    state: Option<State>,
) {
    run_mode_utf16(
        registry,
        &cm_core::text::to_u16(string),
        spec,
        callback,
        state,
    )
}

pub fn run_mode_utf16<F: FnMut(&RunToken)>(
    registry: &Registry,
    string: &[u16],
    spec: &Spec,
    mut callback: F,
    state: Option<State>,
) {
    let options = ModeOptions::default();
    let mode = registry.get_mode(&options, spec);
    let lines = split_lines_runmode(string);
    // `(options && options.state) || CodeMirror.startState(mode)`
    let mut state =
        state.unwrap_or_else(|| start_state(&*mode, JsArg::Undefined, JsArg::Undefined));
    for i in 0..lines.len() {
        if i > 0 {
            callback(&RunToken {
                text: vec![0x0a],
                style: None,
                line: None,
                start: None,
            });
        }
        let oracle = LinesOracle { lines: &lines, i };
        let mut stream = StringStream::new(&lines[i], None, Some(&oracle));
        if stream.string.is_empty() && mode.has_blank_line() {
            mode.blank_line(&mut state);
        }
        while !stream.eol() {
            let style = mode.token(&mut stream, &mut state);
            callback(&RunToken {
                text: stream.current().to_vec(),
                style,
                line: Some(i),
                start: Some(stream.start),
            });
            stream.start = stream.pos;
        }
    }
}

/// What [`run_mode_events`] reports: the start of each line (before `blankLine`
/// and before the first token, where `test/mode_test.js` asks `mode.indent`), and
/// each `runMode` callback.
pub enum RunEvent<'a> {
    LineStart {
        line: usize,
        text: &'a [u16],
        mode: &'a Rc<dyn Mode>,
        state: &'a State,
    },
    Token(&'a RunToken),
}

/// `runMode` (same loop as [`run_mode_utf16`]) with explicit editor options and a
/// line-start hook, for the indentation oracle.
pub fn run_mode_events<F: FnMut(RunEvent<'_>)>(
    registry: &Registry,
    options: &ModeOptions,
    string: &[u16],
    spec: &Spec,
    mut callback: F,
) {
    let mode = registry.get_mode(options, spec);
    let lines = split_lines_runmode(string);
    let mut state = start_state(&*mode, JsArg::Undefined, JsArg::Undefined);
    for i in 0..lines.len() {
        if i > 0 {
            callback(RunEvent::Token(&RunToken {
                text: vec![0x0a],
                style: None,
                line: None,
                start: None,
            }));
        }
        callback(RunEvent::LineStart {
            line: i,
            text: &lines[i],
            mode: &mode,
            state: &state,
        });
        let oracle = LinesOracle { lines: &lines, i };
        let mut stream = StringStream::new(&lines[i], None, Some(&oracle));
        if stream.string.is_empty() && mode.has_blank_line() {
            mode.blank_line(&mut state);
        }
        while !stream.eol() {
            let style = mode.token(&mut stream, &mut state);
            callback(RunEvent::Token(&RunToken {
                text: stream.current().to_vec(),
                style,
                line: Some(i),
                start: Some(stream.start),
            }));
            stream.start = stream.pos;
        }
    }
}

/// Convenience: `runMode` collected into `(text, style)` pairs, line breaks
/// included as `("\n", None)`.
pub fn run_mode_collect(
    registry: &Registry,
    string: &str,
    spec: &Spec,
) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    run_mode(
        registry,
        string,
        spec,
        |t| out.push((from_u16(&t.text), t.style.clone())),
        None,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_mode_rules() {
        let r = Registry::with_all_modes();
        assert_eq!(r.resolve_mode(&Spec::from("text/x-toml")).name, "toml");
        assert_eq!(r.resolve_mode(&Spec::from("text/plain")).name, "null");
        assert_eq!(r.resolve_mode(&Spec::from("toml")).name, "toml");
        assert_eq!(r.resolve_mode(&Spec::Null).name, "null");
        // `+xml` / `+json` go through application/xml and application/json.
        assert_eq!(r.resolve_mode(&Spec::from("image/svg+xml")).name, "xml");
        let j = r.resolve_mode(&Spec::from("application/foo+json"));
        assert_eq!(j.name, "javascript");
        assert_eq!(j.options.get("json"), Some(&ConfigValue::Bool(true)));
        // A defined MIME wins over the +json rule.
        assert_eq!(
            r.resolve_mode(&Spec::from("application/ld+json"))
                .options
                .get("jsonld"),
            Some(&ConfigValue::Bool(true))
        );
        assert_eq!(r.resolve_mode(&Spec::from("text/html")).name, "htmlmixed");
        let mut o = ModeSpec::named("text/x-toml");
        o.options.insert("x".into(), ConfigValue::Bool(true));
        let res = r.resolve_mode(&Spec::Obj(o));
        assert_eq!(res.name, "toml");
        assert_eq!(res.options.get("x"), Some(&ConfigValue::Bool(true)));
        // Unknown modes fall back to text/plain, whose mode is named "null".
        assert_eq!(
            r.get_mode(&ModeOptions::default(), &Spec::from("nope"))
                .name(),
            "null"
        );
        assert_eq!(
            r.get_mode(&ModeOptions::default(), &Spec::from("text/x-toml"))
                .name(),
            "toml"
        );
    }

    /// `getMode` copies `modeExtensions[name]` and then `spec.modeProps` onto the mode
    /// (test/test.js "modeProps" sets `{name: "yyy", modeProps: {xxx: "b", enableC: true}}`).
    #[test]
    fn extend_mode_invalid_override() {
        let mut r = Registry::with_all_modes();
        let mut ext = HashMap::new();
        // javascript has no blank_line.
        // Invalid override for a function-valued method
        ext.insert("indent".to_string(), ConfigValue::Num(4.0));
        ext.insert("blankLine".to_string(), ConfigValue::Str("nope".into()));
        r.extend_mode("javascript", ext);

        let m = r.get_mode(&ModeOptions::default(), &Spec::from("javascript"));
        // javascript mode does NOT have a blank_line inner, and we injected a string instead of a function.
        // It should correctly check the type via downcast_ref and report false.
        assert!(!m.has_blank_line());
        assert!(m.has_indent()); // javascript has indent inner, which falls through

        // Add a valid override
        let mut valid_ext = HashMap::new();
        let valid_blank = std::rc::Rc::new(|_: &mut State| Some("blank".to_string()))
            as std::rc::Rc<dyn Fn(&mut State) -> Option<String>>;
        valid_ext.insert(
            "blankLine".to_string(),
            ConfigValue::Obj(std::rc::Rc::new(valid_blank.clone())),
        );
        r.extend_mode("javascript", valid_ext);

        let m2 = r.get_mode(&ModeOptions::default(), &Spec::from("javascript"));
        assert!(m2.has_blank_line());

        // call it and assert the result
        let mut state = m2.start_state(JsArg::Undefined, JsArg::Undefined).unwrap();
        assert_eq!(m2.blank_line(&mut state), Some("blank".to_string()));
    }

    #[test]
    fn extend_mode_and_mode_props() {
        let mut r = Registry::with_all_modes();
        let mut ext = HashMap::new();
        ext.insert("xxx".to_string(), ConfigValue::Str("a".into()));
        ext.insert("fold".to_string(), ConfigValue::Str("indent".into()));
        r.extend_mode("vrml", ext);
        let m = r.get_mode(&ModeOptions::default(), &Spec::from("vrml"));
        assert_eq!(m.prop("xxx"), Some(ConfigValue::Str("a".into())));
        assert_eq!(m.prop("fold"), Some(ConfigValue::Str("indent".into())));
        assert_eq!(m.prop("lineComment"), Some(ConfigValue::Str("#".into())));
        let mut props = HashMap::new();
        props.insert("xxx".to_string(), ConfigValue::Str("b".into()));
        props.insert("enableC".to_string(), ConfigValue::Bool(true));
        let spec = ModeSpec {
            mode_props: Some(props),
            ..ModeSpec::named("vrml")
        };
        let m = r.get_mode(&ModeOptions::default(), &Spec::Obj(spec));
        assert_eq!(m.prop("xxx"), Some(ConfigValue::Str("b".into())));
        assert_eq!(m.prop("enableC"), Some(ConfigValue::Bool(true)));
        assert_eq!(m.prop("nope"), None);
    }

    #[test]
    fn inner_mode_of_htmlmixed() {
        let r = Registry::with_all_modes();
        let mode = r.get_mode(&ModeOptions::default(), &Spec::from("text/html"));
        let mut state = start_state(&*mode, JsArg::Undefined, JsArg::Undefined);
        let line = cm_core::text::to_u16("<style>a{");
        let mut stream = StringStream::new(&line, None, None);
        let mut inner_names = Vec::new();
        while !stream.eol() {
            let mut slot = None;
            read_token(&mode, &mut stream, &mut state, Some(&mut slot)).unwrap();
            inner_names.push(slot.unwrap().name().to_string());
            stream.start = stream.pos;
        }
        // `readToken` asks innerMode before each token: xml until `>` starts css.
        assert_eq!(inner_names, ["xml", "xml", "xml", "css", "css"]);
        assert_eq!(inner_mode(mode.clone(), state.clone()).0.name(), "css");
        // copyState goes through htmlmixed's own copyState.
        assert!(mode.copy_state(&state).is_some());
    }

    #[test]
    fn null_mode_run() {
        let r = Registry::new();
        let toks = run_mode_collect(&r, "ab\r\ncd\r\n", &Spec::from("text/plain"));
        assert_eq!(
            toks,
            vec![
                ("ab".into(), None),
                ("\n".into(), None),
                ("cd".into(), None),
                ("\n".into(), None)
            ]
        );
    }
}
