//! Port of `mode/xml/xml.js` (CodeMirror 5.65.21): the `xml` mode, with the html
//! configuration (`htmlMode: true`). Quirks are kept, e.g. a null style that turns
//! into an error renders as `"null error"`, the close-tag name comparison is
//! case-sensitive even in html mode, and the module-level `type` / `setStyle`
//! scratch variables live on the mode instance.
//!
//! Not ported: overriding the html/xml dictionaries (`autoSelfClosers`,
//! `implicitlyClosed`, `contextGrabbers`, `doNotIndent`) through the spec — only the
//! boolean and numeric options are read. `indent` looks the context's tag name up
//! with `contextGrabbers[name]`, so in JS a tag named like an `Object.prototype`
//! member (`constructor`, ...) finds a function there; that is not reproduced.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::OnceLock;

use cm_core::util::string_stream::{RegexMatch, StringStream};

use crate::js::{self, JsStr, is_js_space, is_js_word};
use crate::{ConfigValue, JsArg, MimeTarget, Mode, ModeOptions, ModeSpec, Registry, State};

/// `htmlConfig` / `xmlConfig` plus the spec's own options.
#[derive(Debug)]
pub struct XmlConfig {
    pub auto_self_closers: HashSet<&'static str>,
    pub implicitly_closed: HashSet<&'static str>,
    pub context_grabbers: HashMap<&'static str, HashSet<&'static str>>,
    pub do_not_indent: HashSet<&'static str>,
    pub allow_unquoted: bool,
    pub allow_missing: bool,
    pub allow_missing_tag_name: bool,
    pub html_mode: bool,
    /// `config.matchClosing === false`
    pub match_closing_false: bool,
    /// `config.multilineTagIndentPastTag !== false`
    pub multiline_tag_indent_past_tag: bool,
    /// `config.multilineTagIndentFactor || 1`
    pub multiline_tag_indent_factor: f64,
    pub align_cdata: bool,
}

fn html_dicts() -> &'static (
    HashSet<&'static str>,
    HashSet<&'static str>,
    HashMap<&'static str, HashSet<&'static str>>,
) {
    static D: OnceLock<(
        HashSet<&'static str>,
        HashSet<&'static str>,
        HashMap<&'static str, HashSet<&'static str>>,
    )> = OnceLock::new();
    D.get_or_init(|| {
        let auto = [
            "area", "base", "br", "col", "command", "embed", "frame", "hr", "img", "input",
            "keygen", "link", "meta", "param", "source", "track", "wbr", "menuitem",
        ];
        let implicit = [
            "dd", "li", "optgroup", "option", "p", "rp", "rt", "tbody", "td", "tfoot", "th", "tr",
        ];
        let grab: &[(&str, &[&str])] = &[
            ("dd", &["dd", "dt"]),
            ("dt", &["dd", "dt"]),
            ("li", &["li"]),
            ("option", &["option", "optgroup"]),
            ("optgroup", &["optgroup"]),
            (
                "p",
                &[
                    "address",
                    "article",
                    "aside",
                    "blockquote",
                    "dir",
                    "div",
                    "dl",
                    "fieldset",
                    "footer",
                    "form",
                    "h1",
                    "h2",
                    "h3",
                    "h4",
                    "h5",
                    "h6",
                    "header",
                    "hgroup",
                    "hr",
                    "menu",
                    "nav",
                    "ol",
                    "p",
                    "pre",
                    "section",
                    "table",
                    "ul",
                ],
            ),
            ("rp", &["rp", "rt"]),
            ("rt", &["rp", "rt"]),
            ("tbody", &["tbody", "tfoot"]),
            ("td", &["td", "th"]),
            ("tfoot", &["tbody"]),
            ("th", &["td", "th"]),
            ("thead", &["tbody", "tfoot"]),
            ("tr", &["tr"]),
        ];
        (
            auto.into_iter().collect(),
            implicit.into_iter().collect(),
            grab.iter()
                .map(|(k, v)| (*k, v.iter().copied().collect()))
                .collect(),
        )
    })
}

impl XmlConfig {
    /// `defineMode("xml", function(editorConf, config_) {...})`: the defaults, then
    /// every own property of `config_`.
    pub fn from_spec(spec: &ModeSpec) -> XmlConfig {
        let html = spec.truthy("htmlMode");
        let (auto, implicit, grab) = html_dicts();
        let opt = |k: &str| spec.get(k);
        let truthy_or = |k: &str, d: bool| opt(k).map_or(d, ConfigValue::truthy);
        XmlConfig {
            auto_self_closers: if html { auto.clone() } else { HashSet::new() },
            implicitly_closed: if html {
                implicit.clone()
            } else {
                HashSet::new()
            },
            context_grabbers: if html { grab.clone() } else { HashMap::new() },
            do_not_indent: if html {
                ["pre"].into_iter().collect()
            } else {
                HashSet::new()
            },
            allow_unquoted: truthy_or("allowUnquoted", html),
            allow_missing: truthy_or("allowMissing", html),
            allow_missing_tag_name: truthy_or("allowMissingTagName", false),
            html_mode: html,
            match_closing_false: opt("matchClosing") == Some(&ConfigValue::Bool(false)),
            multiline_tag_indent_past_tag: opt("multilineTagIndentPastTag")
                != Some(&ConfigValue::Bool(false)),
            multiline_tag_indent_factor: match opt("multilineTagIndentFactor") {
                Some(v) if v.truthy() => v.as_num().unwrap_or(f64::NAN),
                _ => 1.0,
            },
            align_cdata: truthy_or("alignCDATA", false),
        }
    }
}

/// `state.tokenize`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tokenize {
    InText,
    InTag,
    /// `inAttribute(quote)` (`isInAttribute`).
    InAttribute(u16),
    /// `inBlock(style, terminator)`.
    InBlock(&'static str, &'static str),
    Doctype(u32),
}

/// `state.state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateFn {
    Base,
    TagName,
    CloseTagName,
    Close,
    CloseErr,
    Attr,
    AttrEq,
    AttrValue,
    AttrContinued,
}

/// xml.js: `Context`. Never mutated after construction.
#[derive(Debug)]
pub struct Context {
    pub prev: Option<Rc<Context>>,
    pub tag_name: Vec<u16>,
    pub indent: f64,
    pub start_of_line: bool,
    pub no_indent: bool,
}

#[derive(Clone, Debug)]
pub struct XmlState {
    pub tokenize: Tokenize,
    pub state: StateFn,
    pub indented: f64,
    pub tag_name: Option<Vec<u16>>,
    pub tag_start: Option<f64>,
    pub context: Option<Rc<Context>>,
    /// Own property only when `startState` got a non-null `baseIndent`.
    pub base_indent: Option<JsArg>,
    /// Undefined until the first quoted attribute value.
    pub string_start_col: Option<f64>,
}

pub struct XmlMode {
    config: XmlConfig,
    indent_unit: f64,
    /// Module-scope `type` and `setStyle`.
    ty: Cell<Option<&'static str>>,
    set_style: Cell<Option<&'static str>>,
}

const LT: u16 = b'<' as u16;
const GT: u16 = b'>' as u16;

/// `stream.eatWhile(/[\w\._\-]/)`
fn name_char(c: u16) -> bool {
    is_js_word(c) || c == b'.' as u16 || c == b'-' as u16
}

/// `/^[^\s =<>\"\']*[^\s =<>\"\'\/]/`: the longest run of attribute-word
/// units that does not end in `/`.
fn re_word(s: &[u16]) -> Option<RegexMatch> {
    let a = |c: u16| !is_js_space(c) && !matches!(c, 0x3d | 0x3c | 0x3e | 0x22 | 0x27);
    let run = s.iter().take_while(|&&c| a(c)).count();
    (1..=run)
        .rev()
        .find(|&k| s[k - 1] != b'/' as u16)
        .map(|k| RegexMatch { index: 0, len: k })
}

fn lower(s: &[u16]) -> String {
    js::from_u16(&js::to_lower(s))
}

impl XmlMode {
    pub fn new(options: &ModeOptions, spec: &ModeSpec) -> XmlMode {
        XmlMode {
            config: XmlConfig::from_spec(spec),
            indent_unit: options.indent_unit as f64,
            ty: Cell::new(None),
            set_style: Cell::new(None),
        }
    }

    /// xml.js: `inText`
    fn in_text(&self, stream: &mut StringStream<'_>, state: &mut XmlState) -> JsStr {
        let ch = stream.next();
        if ch == Some(LT) {
            if stream.eat(b'!' as u16).is_some() {
                if stream.eat(b'[' as u16).is_some() {
                    if stream.match_str("CDATA[", true, false) {
                        state.tokenize = Tokenize::InBlock("atom", "]]>");
                        return self.tokenize(stream, state);
                    }
                    JsStr::Null
                } else if stream.match_str("--", true, false) {
                    state.tokenize = Tokenize::InBlock("comment", "-->");
                    self.tokenize(stream, state)
                } else if stream.match_str("DOCTYPE", true, true) {
                    stream.eat_while(name_char);
                    state.tokenize = Tokenize::Doctype(1);
                    self.tokenize(stream, state)
                } else {
                    JsStr::Null
                }
            } else if stream.eat(b'?' as u16).is_some() {
                stream.eat_while(name_char);
                state.tokenize = Tokenize::InBlock("meta", "?>");
                JsStr::s("meta")
            } else {
                self.ty.set(Some(if stream.eat(b'/' as u16).is_some() {
                    "closeTag"
                } else {
                    "openTag"
                }));
                state.tokenize = Tokenize::InTag;
                JsStr::s("tag bracket")
            }
        } else if ch == Some(b'&' as u16) {
            let ok = if stream.eat(b'#' as u16).is_some() {
                if stream.eat(b'x' as u16).is_some() {
                    stream.eat_while(js::is_hex) && stream.eat(b';' as u16).is_some()
                } else {
                    stream.eat_while(js::is_digit) && stream.eat(b';' as u16).is_some()
                }
            } else {
                stream.eat_while(|c| {
                    is_js_word(c) || c == b'.' as u16 || c == b'-' as u16 || c == b':' as u16
                }) && stream.eat(b';' as u16).is_some()
            };
            JsStr::s(if ok { "atom" } else { "error" })
        } else {
            stream.eat_while(|c| c != b'&' as u16 && c != LT);
            JsStr::Null
        }
    }

    /// xml.js: `inTag`
    fn in_tag(&self, stream: &mut StringStream<'_>, state: &mut XmlState) -> JsStr {
        let ch = stream.next();
        if ch == Some(GT) || (ch == Some(b'/' as u16) && stream.eat(GT).is_some()) {
            state.tokenize = Tokenize::InText;
            self.ty.set(Some(if ch == Some(GT) {
                "endTag"
            } else {
                "selfcloseTag"
            }));
            JsStr::s("tag bracket")
        } else if ch == Some(b'=' as u16) {
            self.ty.set(Some("equals"));
            JsStr::Null
        } else if ch == Some(LT) {
            state.tokenize = Tokenize::InText;
            state.state = StateFn::Base;
            state.tag_name = None;
            state.tag_start = None;
            let next = self.tokenize(stream, state);
            if next.truthy() {
                next.concat(" tag error")
            } else {
                JsStr::s("tag error")
            }
        } else if let Some(q @ (0x22 | 0x27)) = ch {
            state.tokenize = Tokenize::InAttribute(q);
            state.string_start_col = Some(stream.column() as f64);
            self.tokenize(stream, state)
        } else {
            stream.match_with(re_word, true);
            JsStr::s("word")
        }
    }

    /// `state.tokenize(stream, state)`.
    fn tokenize(&self, stream: &mut StringStream<'_>, state: &mut XmlState) -> JsStr {
        match state.tokenize {
            Tokenize::InText => self.in_text(stream, state),
            Tokenize::InTag => self.in_tag(stream, state),
            // xml.js: `inAttribute` closure
            Tokenize::InAttribute(quote) => {
                while !stream.eol() {
                    if stream.next() == Some(quote) {
                        state.tokenize = Tokenize::InTag;
                        break;
                    }
                }
                JsStr::s("string")
            }
            // xml.js: `inBlock` closure
            Tokenize::InBlock(style, terminator) => {
                while !stream.eol() {
                    if stream.match_str(terminator, true, false) {
                        state.tokenize = Tokenize::InText;
                        break;
                    }
                    stream.next();
                }
                JsStr::s(style)
            }
            // xml.js: `doctype` closure. The JS recurses into a fresh closure on every
            // `<` / inner `>`; the loop below continues the same scan instead.
            Tokenize::Doctype(mut depth) => {
                while let Some(ch) = stream.next() {
                    if ch == LT {
                        depth += 1;
                        state.tokenize = Tokenize::Doctype(depth);
                    } else if ch == GT {
                        if depth == 1 {
                            state.tokenize = Tokenize::InText;
                            break;
                        }
                        depth -= 1;
                        state.tokenize = Tokenize::Doctype(depth);
                    }
                }
                JsStr::s("meta")
            }
        }
    }

    /// xml.js: `popContext`
    fn pop_context(state: &mut XmlState) {
        if let Some(c) = state.context.take() {
            state.context = c.prev.clone();
        }
    }

    /// xml.js: `maybePopContext`
    fn maybe_pop_context(&self, state: &mut XmlState, next_tag_name: Option<&[u16]>) {
        loop {
            let Some(cx) = &state.context else { return };
            let parent = lower(&cx.tag_name);
            let Some(grab) = self.config.context_grabbers.get(parent.as_str()) else {
                return;
            };
            let Some(next) = next_tag_name else { return };
            if !grab.contains(lower(next).as_str()) {
                return;
            }
            Self::pop_context(state);
        }
    }

    /// xml.js: `new Context(state, tagName, startOfLine)`
    fn push_context(&self, state: &mut XmlState, tag_name: Option<Vec<u16>>, start_of_line: bool) {
        let tag_name = tag_name.unwrap_or_default();
        let no_indent = self
            .config
            .do_not_indent
            .contains(js::from_u16(&tag_name).as_str())
            || state.context.as_ref().is_some_and(|c| c.no_indent);
        state.context = Some(Rc::new(Context {
            prev: state.context.take(),
            tag_name,
            indent: state.indented,
            start_of_line,
            no_indent,
        }));
    }

    /// The state functions (`baseState`, `tagNameState`, ...).
    fn apply(
        &self,
        f: StateFn,
        ty: &str,
        stream: &mut StringStream<'_>,
        state: &mut XmlState,
    ) -> StateFn {
        let c = &self.config;
        match f {
            StateFn::Base => match ty {
                "openTag" => {
                    state.tag_start = Some(stream.column() as f64);
                    StateFn::TagName
                }
                "closeTag" => StateFn::CloseTagName,
                _ => StateFn::Base,
            },
            StateFn::TagName => {
                if ty == "word" {
                    state.tag_name = Some(stream.current().to_vec());
                    self.set_style.set(Some("tag"));
                    StateFn::Attr
                } else if c.allow_missing_tag_name && ty == "endTag" {
                    self.set_style.set(Some("tag bracket"));
                    self.apply(StateFn::Attr, ty, stream, state)
                } else {
                    self.set_style.set(Some("error"));
                    StateFn::TagName
                }
            }
            StateFn::CloseTagName => {
                if ty == "word" {
                    let tag_name = stream.current().to_vec();
                    if let Some(cx) = &state.context {
                        if cx.tag_name != tag_name
                            && c.implicitly_closed.contains(lower(&cx.tag_name).as_str())
                        {
                            Self::pop_context(state);
                        }
                    }
                    if state
                        .context
                        .as_ref()
                        .is_some_and(|cx| cx.tag_name == tag_name)
                        || c.match_closing_false
                    {
                        self.set_style.set(Some("tag"));
                        StateFn::Close
                    } else {
                        self.set_style.set(Some("tag error"));
                        StateFn::CloseErr
                    }
                } else if c.allow_missing_tag_name && ty == "endTag" {
                    self.set_style.set(Some("tag bracket"));
                    self.apply(StateFn::Close, ty, stream, state)
                } else {
                    self.set_style.set(Some("error"));
                    StateFn::CloseErr
                }
            }
            StateFn::Close => {
                if ty != "endTag" {
                    self.set_style.set(Some("error"));
                    return StateFn::Close;
                }
                Self::pop_context(state);
                StateFn::Base
            }
            StateFn::CloseErr => {
                self.set_style.set(Some("error"));
                self.apply(StateFn::Close, ty, stream, state)
            }
            StateFn::Attr => {
                if ty == "word" {
                    self.set_style.set(Some("attribute"));
                    StateFn::AttrEq
                } else if ty == "endTag" || ty == "selfcloseTag" {
                    let tag_name = state.tag_name.take();
                    let tag_start = state.tag_start.take();
                    let auto = tag_name
                        .as_ref()
                        .is_some_and(|t| c.auto_self_closers.contains(lower(t).as_str()));
                    self.maybe_pop_context(state, tag_name.as_deref());
                    if !(ty == "selfcloseTag" || auto) {
                        let sol = tag_start == Some(state.indented);
                        self.push_context(state, tag_name, sol);
                    }
                    StateFn::Base
                } else {
                    self.set_style.set(Some("error"));
                    StateFn::Attr
                }
            }
            StateFn::AttrEq => {
                if ty == "equals" {
                    return StateFn::AttrValue;
                }
                if !c.allow_missing {
                    self.set_style.set(Some("error"));
                }
                self.apply(StateFn::Attr, ty, stream, state)
            }
            StateFn::AttrValue => {
                if ty == "string" {
                    return StateFn::AttrContinued;
                }
                if ty == "word" && c.allow_unquoted {
                    self.set_style.set(Some("string"));
                    return StateFn::Attr;
                }
                self.set_style.set(Some("error"));
                self.apply(StateFn::Attr, ty, stream, state)
            }
            StateFn::AttrContinued => {
                if ty == "string" {
                    return StateFn::AttrContinued;
                }
                self.apply(StateFn::Attr, ty, stream, state)
            }
        }
    }

    /// The JS mode's `token`, with the JS null/undefined/string style value.
    pub fn token_js(&self, stream: &mut StringStream<'_>, state: &mut XmlState) -> JsStr {
        if state.tag_name.is_none() && stream.sol() {
            state.indented = stream.indentation() as f64;
        }
        if stream.eat_space() {
            return JsStr::Null;
        }
        self.ty.set(None);
        let mut style = self.tokenize(stream, state);
        let ty = self.ty.get();
        if (style.truthy() || ty.is_some()) && !style.is("comment") {
            self.set_style.set(None);
            // `type || style`
            let t: String = match ty {
                Some(t) => t.to_string(),
                None => style.as_js_string(),
            };
            state.state = self.apply(state.state, &t, stream, state);
            if let Some(set) = self.set_style.get() {
                style = if set == "error" {
                    style.concat(" error")
                } else {
                    JsStr::s(set)
                };
            }
        }
        style
    }

    /// The JS mode's `indent`.
    pub fn indent_js(
        &self,
        state: &XmlState,
        text_after: &[u16],
        full_line: &[u16],
    ) -> Option<f64> {
        let c = &self.config;
        let mut context = state.context.clone();
        // Indent multi-line strings (e.g. css).
        if let Tokenize::InAttribute(_) = state.tokenize {
            return Some(if state.tag_start == Some(state.indented) {
                state.string_start_col.map_or(f64::NAN, |c| c + 1.0)
            } else {
                state.indented + self.indent_unit
            });
        }
        if context.as_ref().is_some_and(|c| c.no_indent) {
            return None;
        }
        if state.tokenize != Tokenize::InTag && state.tokenize != Tokenize::InText {
            return Some(js::space_len(full_line) as f64);
        }
        // Indent the starts of attribute names.
        if let Some(tag_name) = &state.tag_name {
            if !tag_name.is_empty() {
                // `null + n` is n in JS.
                let start = state.tag_start.unwrap_or(0.0);
                return Some(if c.multiline_tag_indent_past_tag {
                    start + tag_name.len() as f64 + 2.0
                } else {
                    start + self.indent_unit * c.multiline_tag_indent_factor
                });
            }
        }
        if c.align_cdata
            && cm_core::text::index_of(text_after, &js::to_u16("<![CDATA["), 0).is_some()
        {
            return Some(0.0);
        }
        // `/^<(\/)?([\w_:\.-]*)/.exec(textAfter)`
        if text_after.first() == Some(&LT) {
            let close = text_after.get(1) == Some(&(b'/' as u16));
            let s = if close { 2 } else { 1 };
            let n = text_after[s..]
                .iter()
                .take_while(|&&c| {
                    is_js_word(c) || c == b':' as u16 || c == b'.' as u16 || c == b'-' as u16
                })
                .count();
            let name = &text_after[s..s + n];
            if close {
                // Closing tag spotted
                while let Some(cx) = context.clone() {
                    if cx.tag_name == name {
                        context = cx.prev.clone();
                        break;
                    } else if c.implicitly_closed.contains(lower(&cx.tag_name).as_str()) {
                        context = cx.prev.clone();
                    } else {
                        break;
                    }
                }
            } else {
                // Opening tag spotted
                while let Some(cx) = context.clone() {
                    match c.context_grabbers.get(lower(&cx.tag_name).as_str()) {
                        Some(g) if g.contains(lower(name).as_str()) => context = cx.prev.clone(),
                        _ => break,
                    }
                }
            }
        }
        while let Some(cx) = context.clone() {
            if cx.prev.is_some() && !cx.start_of_line {
                context = cx.prev.clone();
            } else {
                break;
            }
        }
        match context {
            Some(cx) => Some(cx.indent + self.indent_unit),
            // `state.baseIndent || 0`: a Pass base indent is returned as is.
            None => match state.base_indent {
                Some(JsArg::Pass) => None,
                Some(a) => Some(a.or_zero()),
                None => Some(0.0),
            },
        }
    }

    pub fn start_state_js(&self, base_indent: JsArg) -> XmlState {
        XmlState {
            tokenize: Tokenize::InText,
            state: StateFn::Base,
            indented: base_indent.or_zero(),
            tag_name: None,
            tag_start: None,
            context: None,
            base_indent: base_indent.is_some().then_some(base_indent),
            string_start_col: None,
        }
    }
}

impl Mode for XmlMode {
    fn name(&self) -> &str {
        "xml"
    }
    fn start_state(&self, a1: JsArg, _a2: JsArg) -> Option<State> {
        Some(State::new(self.start_state_js(a1)))
    }
    fn token(&self, stream: &mut StringStream<'_>, state: &mut State) -> Option<String> {
        self.token_js(stream, state.get_mut::<XmlState>())
            .into_style()
    }
    fn indent(&self, state: &State, text_after: &[u16], line: &[u16]) -> Option<f64> {
        self.indent_js(state.get::<XmlState>(), text_after, line)
    }
    fn has_indent(&self) -> bool {
        true
    }
    /// `electricInput: /<\/[\s\w:]+>$/`
    fn electric_input(&self, text: &[u16]) -> Option<bool> {
        let Some((&last, rest)) = text.split_last() else {
            return Some(false);
        };
        if last != GT {
            return Some(false);
        }
        let n = rest
            .iter()
            .rev()
            .take_while(|&&c| is_js_space(c) || is_js_word(c) || c == b':' as u16)
            .count();
        Some(
            n > 0
                && rest.len() >= n + 2
                && rest[rest.len() - n - 2..rest.len() - n] == [LT, b'/' as u16],
        )
    }
    fn block_comment_start(&self) -> Option<&str> {
        Some("<!--")
    }
    fn block_comment_end(&self) -> Option<&str> {
        Some("-->")
    }
    fn helper_type(&self) -> Option<&str> {
        Some(if self.config.html_mode { "html" } else { "xml" })
    }
    fn prop(&self, name: &str) -> Option<ConfigValue> {
        match name {
            "configuration" | "helperType" => Some(ConfigValue::Str(
                if self.config.html_mode { "html" } else { "xml" }.into(),
            )),
            "blockCommentStart" => Some(ConfigValue::Str("<!--".into())),
            "blockCommentEnd" => Some(ConfigValue::Str("-->".into())),
            _ => None,
        }
    }
}

/// xml.js: `skipAttribute(state)`.
pub fn skip_attribute(state: &mut State) {
    let s = state.get_mut::<XmlState>();
    if s.state == StateFn::AttrValue {
        s.state = StateFn::Attr;
    }
}

/// xml.js: `xmlCurrentTag(state)`. `close` reads `state.type`, which the mode never
/// sets, so it is always false.
pub fn xml_current_tag(state: &State) -> Option<(Vec<u16>, bool)> {
    let s = state.get::<XmlState>();
    s.tag_name
        .clone()
        .filter(|t| !t.is_empty())
        .map(|t| (t, false))
}

/// xml.js: `xmlCurrentContext(state)`: tag names, outermost first.
pub fn xml_current_context(state: &State) -> Vec<Vec<u16>> {
    let mut out = Vec::new();
    let mut cx = state.get::<XmlState>().context.clone();
    while let Some(c) = cx {
        out.push(c.tag_name.clone());
        cx = c.prev.clone();
    }
    out.reverse();
    out
}

/// `defineMode("xml", ...)`, `defineMIME("text/xml" | "application/xml", "xml")`, and
/// `text/html` -> `{name: "xml", htmlMode: true}` unless already defined.
pub fn register(r: &mut Registry) {
    r.define_mode(
        "xml",
        Rc::new(|_, options, spec| Rc::new(XmlMode::new(options, spec)) as Rc<dyn Mode>),
    );
    r.define_mime("text/xml", MimeTarget::Name("xml".into()));
    r.define_mime("application/xml", MimeTarget::Name("xml".into()));
    if !r.has_mime("text/html") {
        r.define_mime(
            "text/html",
            MimeTarget::Spec(ModeSpec::named("xml").with("htmlMode", ConfigValue::Bool(true))),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cm_core::text::to_u16;

    #[test]
    fn word_regex() {
        assert_eq!(re_word(&to_u16("abc/>")).map(|m| m.len), Some(3));
        assert_eq!(re_word(&to_u16("a/b/ x")).map(|m| m.len), Some(3));
        assert_eq!(re_word(&to_u16("//>")), None);
        assert_eq!(re_word(&to_u16("=x")), None);
    }

    #[test]
    fn electric() {
        let m = XmlMode::new(&ModeOptions::default(), &ModeSpec::named("xml"));
        assert_eq!(m.electric_input(&to_u16("  </a b>")), Some(true));
        assert_eq!(m.electric_input(&to_u16("</>")), Some(false));
        assert_eq!(m.electric_input(&to_u16("<a>")), Some(false));
    }
}
