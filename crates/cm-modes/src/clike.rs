//! Port of `mode/clike/clike.js` (CodeMirror 5.65.21): the `clike` mode and every
//! `def(...)` configuration of the file — C, C++, Java, C#, Scala, Kotlin, GLSL
//! shaders, nesC, Objective-C, Objective-C++, Squirrel and Ceylon — with their MIME
//! types, `hintWords` helpers and `modeProps`.
//!
//! Quirks are kept on purpose; each function cites the JS function it ports.
//!
//! * `parserConfig` options are [`ConfigValue`]s of a [`ModeSpec`], as upstream passes
//!   them through `defineMIME`. Word tables are [`Words`] (`words(str)` objects, or a
//!   predicate such as `cTypes`), regexps are [`RegExp`]s, `hooks` is a [`Hooks`] and
//!   `isReservedIdentifier` a [`Predicate`].
//! * `curPunc` / `isDefKeyword` are closure variables that `token` resets before each
//!   tokenizer call; here they are a [`Scratch`] local to one `token` call.
//! * `state.context` objects are shared by shallow `copyState` copies, and `token`
//!   sets `ctx.align` in place: [`Context`] is behind `Rc` with `align` in a `Cell`.
//! * `state.tokenize` holds a JS function, some of them closures (`tokenString(quote)`,
//!   `tokenNestedComment(depth)`, `tokenKotlinString(triple)`,
//!   `tokenCeylonString(type)`); [`Tokenize`] enumerates them.
//! * Ceylon's `stringTokenizer` is a variable of the module scope, shared by every
//!   clike mode instance. Here it lives in the [`Env`] that [`register`] creates, so it
//!   is shared by every mode of one [`Registry`] (one registry = one load of the file).
//! * Regexps are [`RegExp`]s compiled from the upstream sources.

use std::any::Any;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use cm_core::text::{is_js_space, is_js_word, to_u16};
use cm_core::util::string_stream::StringStream;

use crate::{ConfigValue, JsArg, MimeTarget, Mode, ModeOptions, ModeSpec, RegExp, Registry, State};

fn re(source: &str) -> RegExp {
    RegExp::new(source, "").expect("clike.js regexps compile")
}

const fn u(c: char) -> u16 {
    c as u16
}

// --------------------------------------------------------------------------------
// Configuration values

/// A named predicate over a token (`cTypes`, `objCTypes`, Ceylon's `types`,
/// `cIsReservedIdentifier`). The name is the JS function's `name`.
#[derive(Clone, Copy)]
pub struct Predicate {
    pub name: &'static str,
    pub f: fn(&[u16]) -> bool,
}

impl std::fmt::Debug for Predicate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Predicate({})", self.name)
    }
}

/// A word table: `words(str)` or a function (`contains(words, word)`).
#[derive(Clone, Debug)]
pub enum Words {
    /// `words(str)`: the own keys in insertion order (duplicates dropped, `""` kept
    /// where `split(" ")` produces it).
    Set {
        keys: Vec<String>,
        set: HashSet<Vec<u16>>,
    },
    Fn(Predicate),
}

impl Words {
    /// `words(str)`.
    pub fn split(s: &str) -> Words {
        let mut keys = Vec::new();
        let mut set = HashSet::new();
        for w in s.split(' ') {
            if set.insert(to_u16(w)) {
                keys.push(w.to_string());
            }
        }
        Words::Set { keys, set }
    }
    /// `{}`.
    pub fn empty() -> Words {
        Words::Set {
            keys: Vec::new(),
            set: HashSet::new(),
        }
    }
    /// `contains(words, word)`.
    pub fn contains(&self, word: &[u16]) -> bool {
        match self {
            Words::Set { set, .. } => set.contains(word),
            Words::Fn(p) => (p.f)(word),
        }
    }
    /// `for (var prop in obj) if (obj.hasOwnProperty(prop))`: a function has none.
    pub fn keys(&self) -> &[String] {
        match self {
            Words::Set { keys, .. } => keys,
            Words::Fn(_) => &[],
        }
    }
}

/// A `hooks[ch]` function: `false` is `None`.
pub type CharHook = fn(&mut StringStream<'_>, &mut ClikeState, &Env) -> Option<&'static str>;
/// `hooks.token(stream, state, style)`: `undefined` is `None`.
pub type TokenHook =
    fn(&mut StringStream<'_>, &ClikeState, Option<&'static str>) -> Option<&'static str>;
/// `hooks.indent(state, ctx, textAfter, indentUnit)`: a non-number is `None`.
pub type IndentHook = fn(&ClikeState, &Context, &[u16], f64) -> Option<f64>;

/// `parserConfig.hooks`. Each entry keeps the JS function's `name` (a property
/// function's name is its key).
#[derive(Clone, Default)]
pub struct Hooks {
    pub chars: Vec<(u16, &'static str, CharHook)>,
    pub token: Option<(&'static str, TokenHook)>,
    pub indent: Option<(&'static str, IndentHook)>,
}

impl Hooks {
    fn char_hook(&self, ch: u16) -> Option<CharHook> {
        self.chars.iter().find(|h| h.0 == ch).map(|h| h.2)
    }
    /// `Object.keys(hooks)` with each function's `name`, sorted by key.
    pub fn names(&self) -> Vec<(String, &'static str)> {
        let mut v: Vec<(String, &'static str)> = self
            .chars
            .iter()
            .map(|(c, n, _)| (String::from_utf16_lossy(&[*c]), *n))
            .collect();
        if let Some((n, _)) = self.token {
            v.push(("token".into(), n));
        }
        if let Some((n, _)) = self.indent {
            v.push(("indent".into(), n));
        }
        v.sort();
        v
    }
}

impl std::fmt::Debug for Hooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Hooks({:?})", self.names())
    }
}

/// `namespaceSeparator`: what `stream.match` accepts.
#[derive(Clone, Debug)]
pub enum Pattern {
    Str(String),
    Re(RegExp),
}

// --------------------------------------------------------------------------------
// State

/// `Context.type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtxType {
    Top,
    Statement,
    /// `"}"`
    Brace,
    /// `"]"`
    Bracket,
    /// `")"`
    Paren,
}

impl CtxType {
    pub fn as_str(self) -> &'static str {
        match self {
            CtxType::Top => "top",
            CtxType::Statement => "statement",
            CtxType::Brace => "}",
            CtxType::Bracket => "]",
            CtxType::Paren => ")",
        }
    }
    /// The type as one character (`firstChar == ctx.type`), for the bracket types.
    fn char(self) -> Option<u16> {
        match self {
            CtxType::Brace => Some(u('}')),
            CtxType::Bracket => Some(u(']')),
            CtxType::Paren => Some(u(')')),
            _ => None,
        }
    }
}

/// `Context.info`: `null` for the top context, `undefined` for brackets, the token
/// text for statements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Info {
    Undefined,
    Null,
    Str(Vec<u16>),
}

impl Info {
    fn is(&self, s: &str) -> bool {
        matches!(self, Info::Str(t) if *t == to_u16(s))
    }
    /// `String(info)`, as `regexp.test(info)` converts it.
    fn text(&self) -> Vec<u16> {
        match self {
            Info::Undefined => to_u16("undefined"),
            Info::Null => to_u16("null"),
            Info::Str(t) => t.clone(),
        }
    }
}

/// `Context(indented, column, type, info, align, prev)`.
#[derive(Debug)]
pub struct Context {
    pub indented: f64,
    pub column: f64,
    pub ty: CtxType,
    pub info: Info,
    /// `null` is `None`.
    pub align: Cell<Option<bool>>,
    pub prev: Option<Rc<Context>>,
}

/// Ceylon string kinds (`tokenCeylonString(type)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CeylonType {
    Single,
    Triple,
}

/// The functions `state.tokenize` can hold (`null` is `None`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tokenize {
    /// `tokenString(quote)`
    String(u16),
    /// `tokenComment`
    Comment,
    /// `cppHook` (a preprocessor line continued with `\`)
    CppHook,
    /// `tokenRawString`
    RawString,
    /// `tokenAtString`
    AtString,
    /// `tokenTripleString`
    TripleString,
    /// `tokenNestedComment(depth)`
    NestedComment(i64),
    /// `tokenKotlinString(tripleString)`
    KotlinString(bool),
    /// `tokenCeylonString(type)`
    CeylonString(CeylonType),
}

/// The clike state (`startState` plus the properties the tokenizers add).
#[derive(Clone, Debug)]
pub struct ClikeState {
    pub tokenize: Option<Tokenize>,
    pub context: Rc<Context>,
    pub indented: f64,
    pub start_of_line: bool,
    /// `null` is `None`.
    pub prev_token: Option<String>,
    /// `typeBefore` returns `true` or `undefined`; absent reads as `undefined`.
    pub type_at_end_of_line: bool,
    pub cpp11_raw_string_delim: Option<Vec<u16>>,
}

impl ClikeState {
    fn prev_is(&self, s: &str) -> bool {
        self.prev_token.as_deref() == Some(s)
    }
}

// --------------------------------------------------------------------------------
// The module scope

/// What the module scope of `clike.js` holds for the tokenizers: Ceylon's
/// `stringTokenizer` and the regexp literals of the module-level functions.
pub struct Env {
    /// `var stringTokenizer = null`
    pub ceylon_string_tokenizer: Cell<Option<CeylonType>>,
    re_type_before: RegExp,
    re_op_comment: RegExp,
    re_comma_eol: RegExp,
    re_paren_ahead: RegExp,
    re_case_default: RegExp,
    re_allman: RegExp,
    re_cpp_one_char: RegExp,
    re_cpp_comment: RegExp,
    re_cpp14: RegExp,
    re_raw_prefix: RegExp,
    re_raw_open: RegExp,
    re_unicode_prefix: RegExp,
    re_quote_ahead: RegExp,
    re_ctor: RegExp,
    re_c_type: RegExp,
    re_meta_word: RegExp,
    re_java_triple: RegExp,
    re_scala_char: RegExp,
    re_atom_word: RegExp,
}

impl Env {
    pub fn new() -> Env {
        Env {
            ceylon_string_tokenizer: Cell::new(None),
            re_type_before: re(r"\S(?:[^- ]>|[*\]])\s*$|\*$"),
            re_op_comment: re(r"^\/[\/*]"),
            re_comma_eol: re(r"^\s*(?:\/\/.*)?$"),
            re_paren_ahead: re(r"^\s*\("),
            re_case_default: re(r"^(?:case|default)\b"),
            re_allman: re(r"[{(]"),
            re_cpp_one_char: re(r"^.$"),
            re_cpp_comment: re(r"^\/[\/\*]"),
            re_cpp14: re(r"[\w\.']"),
            re_raw_prefix: re(r"^(?:R|u8R|uR|UR|LR)"),
            re_raw_open: re(r#"^"([^\s\\()]{0,16})\("#),
            re_unicode_prefix: re(r"^(?:u8|u|U|L)"),
            re_quote_ahead: re(r#"^["']"#),
            re_ctor: re(r"(\w+)::~?(\w+)$"),
            re_c_type: re(r".+_t$"),
            re_meta_word: re(r"[\w\$_]"),
            re_java_triple: re(r#"""$"#),
            re_scala_char: re(r"^(\\[^'\s]+|[^\\'])'"),
            re_atom_word: re(r"[\w\$_\xa1-\uffff]"),
        }
    }
}

impl Default for Env {
    fn default() -> Self {
        Env::new()
    }
}

thread_local! {
    /// The regexps that the word predicates (`cTypes`, ...) use: they get no [`Env`].
    static PREDICATE_ENV: Env = Env::new();
}

fn eat_while_re(stream: &mut StringStream<'_>, re: &RegExp) -> bool {
    stream.eat_while(|c| re.test(&[c]))
}

// --------------------------------------------------------------------------------
// The mode

/// `curPunc` values: a punctuation character or `"newstatement"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Punc {
    Char(u16),
    NewStatement,
}

/// `curPunc` and `isDefKeyword` (`null` at the start of each tokenizer call).
#[derive(Default)]
struct Scratch {
    cur_punc: Option<Punc>,
    is_def_keyword: bool,
}

fn opt<T: 'static>(spec: &ModeSpec, key: &str) -> Option<Rc<T>> {
    match spec.get(key) {
        Some(ConfigValue::Obj(o)) => Rc::downcast::<T>(o.clone() as Rc<dyn Any>).ok(),
        _ => None,
    }
}

fn words_opt(spec: &ModeSpec, key: &str) -> Rc<Words> {
    opt::<Words>(spec, key).unwrap_or_else(|| Rc::new(Words::empty()))
}

fn re_opt(spec: &ModeSpec, key: &str, default: &str, flags: &str) -> RegExp {
    opt::<RegExp>(spec, key)
        .map(|r| (*r).clone())
        .unwrap_or_else(|| RegExp::new(default, flags).expect("clike.js regexps compile"))
}

/// The mode object `defineMode("clike", ...)` returns, with its closure variables.
pub struct Clike {
    env: Rc<Env>,
    indent_unit: f64,
    statement_indent_unit: f64,
    dont_align_calls: bool,
    keywords: Rc<Words>,
    types: Rc<Words>,
    builtin: Rc<Words>,
    block_keywords: Rc<Words>,
    def_keywords: Rc<Words>,
    atoms: Rc<Words>,
    hooks: Rc<Hooks>,
    multi_line_strings: bool,
    indent_statements: bool,
    namespace_separator: Option<Pattern>,
    is_punctuation_char: RegExp,
    number_start: RegExp,
    number: RegExp,
    is_operator_char: RegExp,
    is_identifier_char: RegExp,
    is_reserved_identifier: Option<Predicate>,
    type_first_definitions: bool,
    dont_indent_statements: Option<RegExp>,
    allman_indentation: bool,
    style_defs_false: bool,
    electric_input: RegExp,
}

impl Clike {
    pub fn new(env: Rc<Env>, config: &ModeOptions, spec: &ModeSpec) -> Clike {
        let indent_unit = config.indent_unit as f64;
        let statement_indent_unit = match spec.get("statementIndentUnit") {
            Some(v) if v.truthy() => v.as_num().unwrap_or(indent_unit),
            _ => indent_unit,
        };
        let indent_switch = spec.get("indentSwitch") != Some(&ConfigValue::Bool(false));
        let namespace_separator = match spec.get("namespaceSeparator") {
            Some(ConfigValue::Str(s)) => Some(Pattern::Str(s.clone())),
            Some(ConfigValue::Obj(_)) => {
                opt::<RegExp>(spec, "namespaceSeparator").map(|r| Pattern::Re((*r).clone()))
            }
            _ => None,
        };
        Clike {
            env,
            indent_unit,
            statement_indent_unit,
            dont_align_calls: spec.truthy("dontAlignCalls"),
            keywords: words_opt(spec, "keywords"),
            types: words_opt(spec, "types"),
            builtin: words_opt(spec, "builtin"),
            block_keywords: words_opt(spec, "blockKeywords"),
            def_keywords: words_opt(spec, "defKeywords"),
            atoms: words_opt(spec, "atoms"),
            hooks: opt::<Hooks>(spec, "hooks").unwrap_or_default(),
            multi_line_strings: spec.truthy("multiLineStrings"),
            indent_statements: spec.get("indentStatements") != Some(&ConfigValue::Bool(false)),
            namespace_separator,
            is_punctuation_char: re_opt(spec, "isPunctuationChar", r"[\[\]{}\(\),;\:\.]", ""),
            number_start: re_opt(spec, "numberStart", r"[\d\.]", ""),
            number: re_opt(
                spec,
                "number",
                r"^(?:0x[a-f\d]+|0b[01]+|(?:\d+\.?\d*|\.\d+)(?:e[-+]?\d+)?)(u|ll?|l|f)?",
                "i",
            ),
            is_operator_char: re_opt(spec, "isOperatorChar", r"[+\-*&%=<>!?|\/]", ""),
            is_identifier_char: re_opt(spec, "isIdentifierChar", r"[\w\$_\xa1-\uffff]", ""),
            is_reserved_identifier: opt::<Predicate>(spec, "isReservedIdentifier").map(|p| *p),
            type_first_definitions: spec.truthy("typeFirstDefinitions"),
            dont_indent_statements: opt::<RegExp>(spec, "dontIndentStatements")
                .map(|r| (*r).clone()),
            allman_indentation: spec.truthy("allmanIndentation"),
            style_defs_false: spec.get("styleDefs") == Some(&ConfigValue::Bool(false)),
            electric_input: if indent_switch {
                re(r"^\s*(?:case .*?:|default:|\{\}?|\})$")
            } else {
                re(r"^\s*[{}]$")
            },
        }
    }

    /// `tokenBase(stream, state)`.
    fn token_base(
        &self,
        stream: &mut StringStream<'_>,
        state: &mut ClikeState,
        sc: &mut Scratch,
    ) -> Option<&'static str> {
        let ch = stream
            .next()
            .expect("token is not called at the end of the line");
        if let Some(hook) = self.hooks.char_hook(ch) {
            let result = hook(stream, state, &self.env);
            if result.is_some() {
                return result;
            }
        }
        if ch == u('"') || ch == u('\'') {
            state.tokenize = Some(Tokenize::String(ch));
            return self.token_string(ch, stream, state);
        }
        if self.number_start.test(&[ch]) {
            stream.back_up(1);
            if stream.match_regexp(&self.number, true).is_some() {
                return Some("number");
            }
            stream.next();
        }
        if self.is_punctuation_char.test(&[ch]) {
            sc.cur_punc = Some(Punc::Char(ch));
            return None;
        }
        if ch == u('/') {
            if stream.eat(u('*')).is_some() {
                state.tokenize = Some(Tokenize::Comment);
                return token_comment(stream, state);
            }
            if stream.eat(u('/')).is_some() {
                stream.skip_to_end();
                return Some("comment");
            }
        }
        if self.is_operator_char.test(&[ch]) {
            while stream
                .match_regexp(&self.env.re_op_comment, false)
                .is_none()
                && stream
                    .eat_fn(|c| self.is_operator_char.test(&[c]))
                    .is_some()
            {}
            return Some("operator");
        }
        eat_while_re(stream, &self.is_identifier_char);
        if let Some(sep) = &self.namespace_separator {
            loop {
                let matched = match sep {
                    Pattern::Str(s) => stream.match_str(s, true, false),
                    Pattern::Re(r) => stream.match_regexp(r, true).is_some(),
                };
                if !matched {
                    break;
                }
                eat_while_re(stream, &self.is_identifier_char);
            }
        }

        let cur = stream.current();
        if self.keywords.contains(cur) {
            if self.block_keywords.contains(cur) {
                sc.cur_punc = Some(Punc::NewStatement);
            }
            if self.def_keywords.contains(cur) {
                sc.is_def_keyword = true;
            }
            return Some("keyword");
        }
        if self.types.contains(cur) {
            return Some("type");
        }
        if self.builtin.contains(cur) || self.is_reserved_identifier.is_some_and(|p| (p.f)(cur)) {
            if self.block_keywords.contains(cur) {
                sc.cur_punc = Some(Punc::NewStatement);
            }
            return Some("builtin");
        }
        if self.atoms.contains(cur) {
            return Some("atom");
        }
        Some("variable")
    }

    /// `tokenString(quote)`: the closure reads `multiLineStrings`.
    fn token_string(
        &self,
        quote: u16,
        stream: &mut StringStream<'_>,
        state: &mut ClikeState,
    ) -> Option<&'static str> {
        let (mut escaped, mut end) = (false, false);
        while let Some(next) = stream.next() {
            if next == quote && !escaped {
                end = true;
                break;
            }
            escaped = !escaped && next == u('\\');
        }
        if end || !(escaped || self.multi_line_strings) {
            state.tokenize = None;
        }
        Some("string")
    }

    /// `state.tokenize(stream, state)`. `cppHook` can return `false` here; that is
    /// `None`, as `null` is (both are falsy wherever `token` reads the style).
    fn call(
        &self,
        t: &Tokenize,
        stream: &mut StringStream<'_>,
        state: &mut ClikeState,
    ) -> Option<&'static str> {
        match *t {
            Tokenize::String(q) => self.token_string(q, stream, state),
            Tokenize::Comment => token_comment(stream, state),
            Tokenize::CppHook => cpp_hook(stream, state, &self.env),
            Tokenize::RawString => token_raw_string(stream, state),
            Tokenize::AtString => token_at_string(stream, state),
            Tokenize::TripleString => token_triple_string(stream, state),
            Tokenize::NestedComment(d) => token_nested_comment(d, stream, state),
            Tokenize::KotlinString(t) => token_kotlin_string(t, stream, state),
            Tokenize::CeylonString(t) => token_ceylon_string(t, stream, state, &self.env),
        }
    }

    /// `typeBefore(stream, state, pos)`.
    fn type_before(&self, stream: &mut StringStream<'_>, state: &ClikeState, pos: usize) -> bool {
        if state.prev_is("variable") || state.prev_is("type") {
            return true;
        }
        if self.env.re_type_before.test(&stream.string[..pos]) {
            return true;
        }
        state.type_at_end_of_line && stream.column() == stream.indentation()
    }

    /// `maybeEOL(stream, state)`.
    fn maybe_eol(&self, stream: &mut StringStream<'_>, state: &mut ClikeState) {
        if self.type_first_definitions && stream.eol() && is_top_scope(&state.context) {
            state.type_at_end_of_line = self.type_before(stream, state, stream.pos);
        }
    }
}

/// `pushContext(state, col, type, info)`.
fn push_context(state: &mut ClikeState, col: f64, ty: CtxType, info: Info) {
    let mut indent = state.indented;
    if state.context.ty == CtxType::Statement && ty != CtxType::Statement {
        indent = state.context.indented;
    }
    state.context = Rc::new(Context {
        indented: indent,
        column: col,
        ty,
        info,
        align: Cell::new(None),
        prev: Some(state.context.clone()),
    });
}

/// `popContext(state)`.
fn pop_context(state: &mut ClikeState) -> Rc<Context> {
    let t = state.context.ty;
    if matches!(t, CtxType::Paren | CtxType::Bracket | CtxType::Brace) {
        state.indented = state.context.indented;
    }
    state.context = state
        .context
        .prev
        .clone()
        .expect("the top context is never popped");
    state.context.clone()
}

/// `isTopScope(context)`.
fn is_top_scope(context: &Rc<Context>) -> bool {
    let mut c = Some(context.clone());
    loop {
        let Some(x) = c else { return true };
        if x.ty == CtxType::Top {
            return true;
        }
        if x.ty == CtxType::Brace && !x.prev.as_ref().is_some_and(|p| p.info.is("namespace")) {
            return false;
        }
        c = x.prev.clone();
    }
}

impl Mode for Clike {
    fn name(&self) -> &str {
        "clike"
    }

    /// `startState(basecolumn)`.
    fn start_state(&self, a1: JsArg, _a2: JsArg) -> Option<State> {
        Some(State::new(ClikeState {
            tokenize: None,
            context: Rc::new(Context {
                indented: a1.or_zero() - self.indent_unit,
                column: 0.0,
                ty: CtxType::Top,
                info: Info::Null,
                align: Cell::new(Some(false)),
                prev: None,
            }),
            indented: 0.0,
            start_of_line: true,
            prev_token: None,
            type_at_end_of_line: false,
            cpp11_raw_string_delim: None,
        }))
    }

    /// `token(stream, state)`.
    fn token(&self, stream: &mut StringStream<'_>, state: &mut State) -> Option<String> {
        let state = state.get_mut::<ClikeState>();
        let mut ctx = state.context.clone();
        if stream.sol() {
            if ctx.align.get().is_none() {
                ctx.align.set(Some(false));
            }
            state.indented = stream.indentation() as f64;
            state.start_of_line = true;
        }
        if stream.eat_space() {
            self.maybe_eol(stream, state);
            return None;
        }
        let mut sc = Scratch::default();
        let mut style = match state.tokenize.clone() {
            Some(t) => self.call(&t, stream, state),
            None => self.token_base(stream, state, &mut sc),
        };
        if style == Some("comment") || style == Some("meta") {
            return style.map(str::to_string);
        }
        if ctx.align.get().is_none() {
            ctx.align.set(Some(true));
        }

        let p = sc.cur_punc;
        let ch = |c: char| p == Some(Punc::Char(u(c)));
        if ch(';')
            || ch(':')
            || (ch(',') && stream.match_regexp(&self.env.re_comma_eol, false).is_some())
        {
            while state.context.ty == CtxType::Statement {
                pop_context(state);
            }
        } else if ch('{') {
            let col = stream.column() as f64;
            push_context(state, col, CtxType::Brace, Info::Undefined);
        } else if ch('[') {
            let col = stream.column() as f64;
            push_context(state, col, CtxType::Bracket, Info::Undefined);
        } else if ch('(') {
            let col = stream.column() as f64;
            push_context(state, col, CtxType::Paren, Info::Undefined);
        } else if ch('}') {
            while ctx.ty == CtxType::Statement {
                ctx = pop_context(state);
            }
            if ctx.ty == CtxType::Brace {
                ctx = pop_context(state);
            }
            while ctx.ty == CtxType::Statement {
                ctx = pop_context(state);
            }
        } else if matches!(p, Some(Punc::Char(c)) if ctx.ty.char() == Some(c)) {
            pop_context(state);
        } else if self.indent_statements
            && (((ctx.ty == CtxType::Brace || ctx.ty == CtxType::Top) && !ch(';'))
                || (ctx.ty == CtxType::Statement && p == Some(Punc::NewStatement)))
        {
            let col = stream.column() as f64;
            let info = Info::Str(stream.current().to_vec());
            push_context(state, col, CtxType::Statement, info);
        }

        if style == Some("variable")
            && (state.prev_is("def")
                || (self.type_first_definitions
                    && self.type_before(stream, state, stream.start)
                    && is_top_scope(&state.context)
                    && stream
                        .match_regexp(&self.env.re_paren_ahead, false)
                        .is_some()))
        {
            style = Some("def");
        }

        if let Some((_, hook)) = self.hooks.token
            && let Some(result) = hook(stream, state, style)
        {
            style = Some(result);
        }

        if style == Some("def") && self.style_defs_false {
            style = Some("variable");
        }

        state.start_of_line = false;
        state.prev_token = if sc.is_def_keyword {
            Some("def".to_string())
        } else {
            match (style, p) {
                (Some(s), _) => Some(s.to_string()),
                (None, Some(Punc::Char(c))) => Some(String::from_utf16_lossy(&[c])),
                (None, Some(Punc::NewStatement)) => Some("newstatement".to_string()),
                (None, None) => None,
            }
        };
        self.maybe_eol(stream, state);
        style.map(str::to_string)
    }

    /// `indent(state, textAfter)`; `None` is `CodeMirror.Pass`.
    fn indent(&self, state: &State, text_after: &[u16], _line: &[u16]) -> Option<f64> {
        let state = state.get::<ClikeState>();
        if state.tokenize.is_some() || state.type_at_end_of_line && is_top_scope(&state.context) {
            return None;
        }
        let mut ctx = state.context.clone();
        let first_char = text_after.first().copied();
        let closing = first_char.is_some() && first_char == ctx.ty.char();
        if ctx.ty == CtxType::Statement && first_char == Some(u('}')) {
            ctx = ctx.prev.clone().expect("a statement has a parent");
        }
        if let Some(dont) = &self.dont_indent_statements {
            while ctx.ty == CtxType::Statement && dont.test(&ctx.info.text()) {
                ctx = ctx.prev.clone().expect("a statement has a parent");
            }
        }
        if let Some((_, hook)) = self.hooks.indent
            && let Some(n) = hook(state, &ctx, text_after, self.indent_unit)
        {
            return Some(n);
        }
        let switch_block = ctx.prev.as_ref().is_some_and(|p| p.info.is("switch"));
        if self.allman_indentation && first_char.is_some_and(|c| self.env.re_allman.test(&[c])) {
            while ctx.ty != CtxType::Top && ctx.ty != CtxType::Brace {
                ctx = ctx.prev.clone().expect("the top context ends the chain");
            }
            return Some(ctx.indented);
        }
        if ctx.ty == CtxType::Statement {
            return Some(
                ctx.indented
                    + if first_char == Some(u('{')) {
                        0.0
                    } else {
                        self.statement_indent_unit
                    },
            );
        }
        if ctx.align.get() == Some(true) && (!self.dont_align_calls || ctx.ty != CtxType::Paren) {
            return Some(ctx.column + if closing { 0.0 } else { 1.0 });
        }
        if ctx.ty == CtxType::Paren && !closing {
            return Some(ctx.indented + self.statement_indent_unit);
        }
        Some(
            ctx.indented
                + if closing { 0.0 } else { self.indent_unit }
                + if !closing && switch_block && !self.env.re_case_default.test(text_after) {
                    self.indent_unit
                } else {
                    0.0
                },
        )
    }

    fn has_indent(&self) -> bool {
        true
    }

    fn electric_input(&self, text: &[u16]) -> Option<bool> {
        Some(self.electric_input.test(text))
    }

    fn line_comment(&self) -> Option<&str> {
        Some("//")
    }
    fn block_comment_start(&self) -> Option<&str> {
        Some("/*")
    }
    fn block_comment_end(&self) -> Option<&str> {
        Some("*/")
    }

    fn prop(&self, name: &str) -> Option<ConfigValue> {
        match name {
            "electricInput" => Some(ConfigValue::Obj(Rc::new(self.electric_input.clone()))),
            "blockCommentStart" => Some(ConfigValue::Str("/*".into())),
            "blockCommentEnd" => Some(ConfigValue::Str("*/".into())),
            "blockCommentContinue" => Some(ConfigValue::Str(" * ".into())),
            "lineComment" => Some(ConfigValue::Str("//".into())),
            "fold" => Some(ConfigValue::Str("brace".into())),
            _ => None,
        }
    }
}

// --------------------------------------------------------------------------------
// Module-level tokenizers and hooks

/// `tokenComment(stream, state)`.
fn token_comment(stream: &mut StringStream<'_>, state: &mut ClikeState) -> Option<&'static str> {
    let mut maybe_end = false;
    while let Some(ch) = stream.next() {
        if ch == u('/') && maybe_end {
            state.tokenize = None;
            break;
        }
        maybe_end = ch == u('*');
    }
    Some("comment")
}

/// `cppHook(stream, state)`: `false` is `None`.
fn cpp_hook(
    stream: &mut StringStream<'_>,
    state: &mut ClikeState,
    env: &Env,
) -> Option<&'static str> {
    if !state.start_of_line {
        return None;
    }
    let mut next = None;
    while let Some(ch) = stream.peek() {
        if ch == u('\\') && stream.match_regexp(&env.re_cpp_one_char, true).is_some() {
            next = Some(Tokenize::CppHook);
            break;
        } else if ch == u('/') && stream.match_regexp(&env.re_cpp_comment, false).is_some() {
            break;
        }
        stream.next();
    }
    state.tokenize = next;
    Some("meta")
}

/// `pointerHook(_stream, state)`.
fn pointer_hook(
    _stream: &mut StringStream<'_>,
    state: &mut ClikeState,
    _env: &Env,
) -> Option<&'static str> {
    if state.prev_is("type") {
        return Some("type");
    }
    None
}

/// `x !== x.toLowerCase()` for a one-unit string `x`.
fn lower_differs(c: u16) -> bool {
    match char::from_u32(c as u32) {
        Some(ch) => {
            let lower: String = ch.to_lowercase().collect();
            lower != ch.to_string()
        }
        None => false, // a lone surrogate has no case mapping
    }
}

/// `x === x.toUpperCase()` for a one-unit string `x`.
fn upper_same(c: u16) -> bool {
    match char::from_u32(c as u32) {
        Some(ch) => {
            let upper: String = ch.to_uppercase().collect();
            upper == ch.to_string()
        }
        None => true,
    }
}

/// `cIsReservedIdentifier(token)`.
pub fn c_is_reserved_identifier(token: &[u16]) -> bool {
    if token.len() < 2 {
        return false;
    }
    if token[0] != u('_') {
        return false;
    }
    token[1] == u('_') || lower_differs(token[1])
}

/// `cpp14Literal(stream)`.
fn cpp14_literal(
    stream: &mut StringStream<'_>,
    _state: &mut ClikeState,
    env: &Env,
) -> Option<&'static str> {
    eat_while_re(stream, &env.re_cpp14);
    Some("number")
}

/// `cpp11StringHook(stream, state)`.
fn cpp11_string_hook(
    stream: &mut StringStream<'_>,
    state: &mut ClikeState,
    env: &Env,
) -> Option<&'static str> {
    stream.back_up(1);
    // Raw strings.
    if stream.match_regexp(&env.re_raw_prefix, true).is_some() {
        let base = stream.pos;
        let m = stream.match_regexp(&env.re_raw_open, true)?;
        let (a, b) = m.group(1).expect("group 1 always participates");
        state.cpp11_raw_string_delim = Some(stream.string[base + a..base + b].to_vec());
        state.tokenize = Some(Tokenize::RawString);
        return token_raw_string(stream, state);
    }
    // Unicode strings/chars.
    if stream.match_regexp(&env.re_unicode_prefix, true).is_some() {
        if stream.match_regexp(&env.re_quote_ahead, false).is_some() {
            return Some("string");
        }
        return None;
    }
    // Ignore this hook.
    stream.next();
    None
}

/// `cppLooksLikeConstructor(word)`.
fn cpp_looks_like_constructor(word: &[u16], env: &Env) -> bool {
    match env.re_ctor.exec(word) {
        Some(m) => m.text(word, 1) == m.text(word, 2),
        None => false,
    }
}

/// `tokenAtString(stream, state)`: C#-style strings where `""` escapes a quote.
fn token_at_string(stream: &mut StringStream<'_>, state: &mut ClikeState) -> Option<&'static str> {
    while let Some(next) = stream.next() {
        if next == u('"') && stream.eat(u('"')).is_none() {
            state.tokenize = None;
            break;
        }
    }
    Some("string")
}

/// `tokenRawString(stream, state)`: `<prefix>"<delim>( anything )<delim>"`.
fn token_raw_string(stream: &mut StringStream<'_>, state: &mut ClikeState) -> Option<&'static str> {
    // `state.cpp11RawStringDelim.replace(/[^\w\s]/g, '\\$&')`
    let delim = state.cpp11_raw_string_delim.clone().unwrap_or_default();
    let mut source = to_u16(r".*?\)");
    for c in delim {
        if !is_js_word(c) && !is_js_space(c) {
            source.push(u('\\'));
        }
        source.push(c);
    }
    source.push(u('"'));
    let re = RegExp::from_utf16(&source, "").expect("an escaped delimiter compiles");
    if stream.match_regexp(&re, true).is_some() {
        state.tokenize = None;
    } else {
        stream.skip_to_end();
    }
    Some("string")
}

/// `tokenTripleString(stream, state)`.
fn token_triple_string(
    stream: &mut StringStream<'_>,
    state: &mut ClikeState,
) -> Option<&'static str> {
    let mut escaped = false;
    while !stream.eol() {
        if !escaped && stream.match_str("\"\"\"", true, false) {
            state.tokenize = None;
            break;
        }
        escaped = stream.next() == Some(u('\\')) && !escaped;
    }
    Some("string")
}

/// `tokenNestedComment(depth)`. The JS recurses through `state.tokenize(stream,
/// state)` in tail position; this loops with the new depth instead.
fn token_nested_comment(
    mut depth: i64,
    stream: &mut StringStream<'_>,
    state: &mut ClikeState,
) -> Option<&'static str> {
    while let Some(ch) = stream.next() {
        if ch == u('*') && stream.eat(u('/')).is_some() {
            if depth == 1 {
                state.tokenize = None;
                break;
            } else {
                depth -= 1;
                state.tokenize = Some(Tokenize::NestedComment(depth));
            }
        } else if ch == u('/') && stream.eat(u('*')).is_some() {
            depth += 1;
            state.tokenize = Some(Tokenize::NestedComment(depth));
        }
    }
    Some("comment")
}

/// `tokenKotlinString(tripleString)`.
fn token_kotlin_string(
    triple_string: bool,
    stream: &mut StringStream<'_>,
    state: &mut ClikeState,
) -> Option<&'static str> {
    let (mut escaped, mut end) = (false, false);
    while !stream.eol() {
        if !triple_string && !escaped && stream.match_str("\"", true, false) {
            end = true;
            break;
        }
        if triple_string && stream.match_str("\"\"\"", true, false) {
            end = true;
            break;
        }
        let next = stream.next();
        if !escaped && next == Some(u('$')) && stream.match_str("{", true, false) {
            stream.skip_to(&[u('}')]);
        }
        escaped = !escaped && next == Some(u('\\')) && !triple_string;
    }
    if end || !triple_string {
        state.tokenize = None;
    }
    Some("string")
}

/// `tokenCeylonString(type)`.
fn token_ceylon_string(
    ty: CeylonType,
    stream: &mut StringStream<'_>,
    state: &mut ClikeState,
    env: &Env,
) -> Option<&'static str> {
    let (mut escaped, mut end) = (false, false);
    while !stream.eol() {
        if !escaped
            && stream.match_str("\"", true, false)
            && (ty == CeylonType::Single || stream.match_str("\"\"", true, false))
        {
            end = true;
            break;
        }
        if !escaped && stream.match_str("``", true, false) {
            env.ceylon_string_tokenizer.set(Some(ty));
            end = true;
            break;
        }
        let next = stream.next();
        escaped = ty == CeylonType::Single && !escaped && next == Some(u('\\'));
    }
    if end {
        state.tokenize = None;
    }
    Some("string")
}

/// `cTypes(identifier)`.
pub fn c_types(identifier: &[u16]) -> bool {
    const BASIC: [&str; 10] = [
        "int", "long", "char", "short", "double", "float", "unsigned", "signed", "void", "bool",
    ];
    BASIC.iter().any(|w| to_u16(w) == identifier)
        || PREDICATE_ENV.with(|e| e.re_c_type.test(identifier))
}

/// `objCTypes(identifier)`.
pub fn obj_c_types(identifier: &[u16]) -> bool {
    const BASIC: [&str; 6] = ["SEL", "instancetype", "id", "Class", "Protocol", "BOOL"];
    c_types(identifier) || BASIC.iter().any(|w| to_u16(w) == identifier)
}

/// Ceylon's `types`: identifiers that start with an uppercase letter.
pub fn ceylon_types(word: &[u16]) -> bool {
    match word.first() {
        Some(&first) => upper_same(first) && lower_differs(first),
        // `"".charAt(0)` is "", which equals both its cases.
        None => false,
    }
}

/// `"@": function(stream) { ...; return "meta" }` (Scala, Kotlin, Ceylon).
fn meta_at_hook(
    stream: &mut StringStream<'_>,
    _state: &mut ClikeState,
    env: &Env,
) -> Option<&'static str> {
    eat_while_re(stream, &env.re_meta_word);
    Some("meta")
}

/// Java `"@"`: don't match the `@interface` keyword.
fn java_at_hook(
    stream: &mut StringStream<'_>,
    _state: &mut ClikeState,
    env: &Env,
) -> Option<&'static str> {
    if stream.match_str("interface", false, false) {
        return None;
    }
    eat_while_re(stream, &env.re_meta_word);
    Some("meta")
}

/// Java `'"'`.
fn java_quote_hook(
    stream: &mut StringStream<'_>,
    state: &mut ClikeState,
    env: &Env,
) -> Option<&'static str> {
    stream.match_regexp(&env.re_java_triple, true)?;
    state.tokenize = Some(Tokenize::TripleString);
    token_triple_string(stream, state)
}

/// C# `"@"`.
fn csharp_at_hook(
    stream: &mut StringStream<'_>,
    state: &mut ClikeState,
    env: &Env,
) -> Option<&'static str> {
    if stream.eat(u('"')).is_some() {
        state.tokenize = Some(Tokenize::AtString);
        return token_at_string(stream, state);
    }
    eat_while_re(stream, &env.re_meta_word);
    Some("meta")
}

/// Scala `'"'`.
fn scala_quote_hook(
    stream: &mut StringStream<'_>,
    state: &mut ClikeState,
    _env: &Env,
) -> Option<&'static str> {
    if !stream.match_str("\"\"", true, false) {
        return None;
    }
    state.tokenize = Some(Tokenize::TripleString);
    token_triple_string(stream, state)
}

/// Scala `"'"`.
fn scala_apos_hook(
    stream: &mut StringStream<'_>,
    _state: &mut ClikeState,
    env: &Env,
) -> Option<&'static str> {
    if stream.match_regexp(&env.re_scala_char, true).is_some() {
        return Some("string-2");
    }
    eat_while_re(stream, &env.re_atom_word);
    Some("atom")
}

/// Scala `"="`.
fn scala_eq_hook(
    stream: &mut StringStream<'_>,
    state: &mut ClikeState,
    _env: &Env,
) -> Option<&'static str> {
    let cx = state.context.clone();
    if cx.ty == CtxType::Brace && cx.align.get() == Some(true) && stream.eat(u('>')).is_some() {
        state.context = Rc::new(Context {
            indented: cx.indented,
            column: cx.column,
            ty: cx.ty,
            info: cx.info.clone(),
            align: Cell::new(None),
            prev: cx.prev.clone(),
        });
        Some("operator")
    } else {
        None
    }
}

/// Scala and Kotlin `"/"`.
fn nested_comment_hook(
    stream: &mut StringStream<'_>,
    state: &mut ClikeState,
    _env: &Env,
) -> Option<&'static str> {
    stream.eat(u('*'))?;
    state.tokenize = Some(Tokenize::NestedComment(1));
    token_nested_comment(1, stream, state)
}

/// Kotlin `'*'`.
fn kotlin_star_hook(
    _stream: &mut StringStream<'_>,
    state: &mut ClikeState,
    _env: &Env,
) -> Option<&'static str> {
    Some(if state.prev_is(".") {
        "variable"
    } else {
        "operator"
    })
}

/// Kotlin `'"'`.
fn kotlin_quote_hook(
    stream: &mut StringStream<'_>,
    state: &mut ClikeState,
    _env: &Env,
) -> Option<&'static str> {
    let triple = stream.match_str("\"\"", true, false);
    state.tokenize = Some(Tokenize::KotlinString(triple));
    token_kotlin_string(triple, stream, state)
}

/// Kotlin `indent(state, ctx, textAfter, indentUnit)`.
fn kotlin_indent(
    state: &ClikeState,
    ctx: &Context,
    text_after: &[u16],
    indent_unit: f64,
) -> Option<f64> {
    let first_char = text_after.first().copied();
    let close = state.prev_is("}") || state.prev_is(")");
    if close && text_after.is_empty() {
        return Some(state.indented);
    }
    if (state.prev_is("operator") && text_after != [u('}')] && state.context.ty != CtxType::Brace)
        || state.prev_is("variable") && first_char == Some(u('.'))
        || close && first_char == Some(u('.'))
    {
        return Some(indent_unit * 2.0 + ctx.indented);
    }
    if ctx.align.get() == Some(true) && ctx.ty == CtxType::Brace {
        let same = first_char.is_some() && state.context.ty.char() == first_char;
        return Some(ctx.indented + if same { 0.0 } else { indent_unit });
    }
    None
}

/// Ceylon `'"'`.
fn ceylon_quote_hook(
    stream: &mut StringStream<'_>,
    state: &mut ClikeState,
    env: &Env,
) -> Option<&'static str> {
    let ty = if stream.match_str("\"\"", true, false) {
        CeylonType::Triple
    } else {
        CeylonType::Single
    };
    state.tokenize = Some(Tokenize::CeylonString(ty));
    token_ceylon_string(ty, stream, state, env)
}

/// Ceylon `` '`' ``.
fn ceylon_backtick_hook(
    stream: &mut StringStream<'_>,
    state: &mut ClikeState,
    env: &Env,
) -> Option<&'static str> {
    let ty = env.ceylon_string_tokenizer.get()?;
    if !stream.match_str("`", true, false) {
        return None;
    }
    state.tokenize = Some(Tokenize::CeylonString(ty));
    env.ceylon_string_tokenizer.set(None);
    token_ceylon_string(ty, stream, state, env)
}

/// Ceylon `"'"`.
fn ceylon_apos_hook(
    stream: &mut StringStream<'_>,
    _state: &mut ClikeState,
    env: &Env,
) -> Option<&'static str> {
    eat_while_re(stream, &env.re_atom_word);
    Some("atom")
}

/// C++ and Objective-C++ `token(stream, state, style)`.
fn cpp_token_hook(
    stream: &mut StringStream<'_>,
    state: &ClikeState,
    style: Option<&'static str>,
) -> Option<&'static str> {
    if style == Some("variable")
        && stream.peek() == Some(u('('))
        && (state.prev_is(";") || state.prev_token.is_none() || state.prev_is("}"))
        && PREDICATE_ENV.with(|e| cpp_looks_like_constructor(stream.current(), e))
    {
        return Some("def");
    }
    None
}

/// Ceylon `token(_stream, state, style)`.
fn ceylon_token_hook(
    _stream: &mut StringStream<'_>,
    state: &ClikeState,
    style: Option<&'static str>,
) -> Option<&'static str> {
    if (style == Some("variable") || style == Some("type")) && state.prev_is(".") {
        return Some("variable-2");
    }
    None
}

// --------------------------------------------------------------------------------
// def(...)

const C_KEYWORDS: &str = "auto if break case register continue return default do sizeof \
static else struct switch extern typedef union for goto while enum const \
volatile inline restrict asm fortran";

const CPP_KEYWORDS: &str = "alignas alignof and and_eq audit axiom bitand bitor catch \
class compl concept constexpr const_cast decltype delete dynamic_cast \
explicit export final friend import module mutable namespace new noexcept \
not not_eq operator or or_eq override private protected public \
reinterpret_cast requires static_assert static_cast template this \
thread_local throw try typeid typename using virtual xor xor_eq";

const OBJC_KEYWORDS: &str = "bycopy byref in inout oneway out self super atomic nonatomic retain copy \
readwrite readonly strong weak assign typeof nullable nonnull null_resettable _cmd \
@interface @implementation @end @protocol @encode @property @synthesize @dynamic @class \
@public @package @private @protected @required @optional @try @catch @finally @import \
@selector @encode @defs @synchronized @autoreleasepool @compatibility_alias @available";

const OBJC_BUILTINS: &str = "FOUNDATION_EXPORT FOUNDATION_EXTERN NS_INLINE NS_FORMAT_FUNCTION  \
NS_RETURNS_RETAINEDNS_ERROR_ENUM NS_RETURNS_NOT_RETAINED NS_RETURNS_INNER_POINTER \
NS_DESIGNATED_INITIALIZER NS_ENUM NS_OPTIONS NS_REQUIRES_NIL_TERMINATION \
NS_ASSUME_NONNULL_BEGIN NS_ASSUME_NONNULL_END NS_SWIFT_NAME NS_REFINED_FOR_SWIFT";

const C_BLOCK_KEYWORDS: &str = "case do else for if switch while struct enum union";
const C_DEF_KEYWORDS: &str = "struct enum union";

fn words(s: &str) -> ConfigValue {
    ConfigValue::Obj(Rc::new(Words::split(s)))
}

fn pred(name: &'static str, f: fn(&[u16]) -> bool) -> ConfigValue {
    ConfigValue::Obj(Rc::new(Words::Fn(Predicate { name, f })))
}

fn regexp(source: &str, flags: &str) -> ConfigValue {
    ConfigValue::Obj(Rc::new(
        RegExp::new(source, flags).expect("clike.js regexps compile"),
    ))
}

fn reserved() -> ConfigValue {
    ConfigValue::Obj(Rc::new(Predicate {
        name: "cIsReservedIdentifier",
        f: c_is_reserved_identifier,
    }))
}

fn hooks(h: Hooks) -> ConfigValue {
    ConfigValue::Obj(Rc::new(h))
}

fn fold(list: &[&str]) -> (String, ConfigValue) {
    (
        "fold".to_string(),
        ConfigValue::Obj(Rc::new(
            list.iter().map(|s| s.to_string()).collect::<Vec<String>>(),
        )),
    )
}

/// `closeBrackets: {pairs?, triples}`.
fn close_brackets(pairs: Option<&str>, triples: &str) -> (String, ConfigValue) {
    let mut o = HashMap::new();
    if let Some(p) = pairs {
        o.insert("pairs".to_string(), ConfigValue::Str(p.to_string()));
    }
    o.insert("triples".to_string(), ConfigValue::Str(triples.to_string()));
    ("closeBrackets".to_string(), ConfigValue::Obj(Rc::new(o)))
}

fn spec(options: Vec<(&str, ConfigValue)>, mode_props: Vec<(String, ConfigValue)>) -> ModeSpec {
    let mut s = ModeSpec::named("clike");
    for (k, v) in options {
        s.options.insert(k.to_string(), v);
    }
    if !mode_props.is_empty() {
        s.mode_props = Some(mode_props.into_iter().collect());
    }
    s
}

fn yes() -> ConfigValue {
    ConfigValue::Bool(true)
}

fn no() -> ConfigValue {
    ConfigValue::Bool(false)
}

fn c_hooks() -> Hooks {
    Hooks {
        chars: vec![
            (u('#'), "cppHook", cpp_hook),
            (u('*'), "pointerHook", pointer_hook),
        ],
        ..Hooks::default()
    }
}

fn cpp_hooks() -> Hooks {
    let mut chars: Vec<(u16, &'static str, CharHook)> = vec![
        (u('#'), "cppHook", cpp_hook),
        (u('*'), "pointerHook", pointer_hook),
        (u('u'), "cpp11StringHook", cpp11_string_hook),
        (u('U'), "cpp11StringHook", cpp11_string_hook),
        (u('L'), "cpp11StringHook", cpp11_string_hook),
        (u('R'), "cpp11StringHook", cpp11_string_hook),
    ];
    for d in '0'..='9' {
        chars.push((u(d), "cpp14Literal", cpp14_literal));
    }
    Hooks {
        chars,
        token: Some(("token", cpp_token_hook)),
        indent: None,
    }
}

fn cpp_hash_only() -> Hooks {
    Hooks {
        chars: vec![(u('#'), "cppHook", cpp_hook)],
        ..Hooks::default()
    }
}

/// Every `def(mimes, mode)` call of the file, in order.
pub fn languages() -> Vec<(Vec<&'static str>, ModeSpec)> {
    let include = || fold(&["brace", "include"]);
    vec![
        (
            vec!["text/x-csrc", "text/x-c", "text/x-chdr"],
            spec(
                vec![
                    ("keywords", words(C_KEYWORDS)),
                    ("types", pred("cTypes", c_types)),
                    ("blockKeywords", words(C_BLOCK_KEYWORDS)),
                    ("defKeywords", words(C_DEF_KEYWORDS)),
                    ("typeFirstDefinitions", yes()),
                    ("atoms", words("NULL true false")),
                    ("isReservedIdentifier", reserved()),
                    ("hooks", hooks(c_hooks())),
                ],
                vec![include()],
            ),
        ),
        (
            vec!["text/x-c++src", "text/x-c++hdr"],
            spec(
                vec![
                    ("keywords", words(&format!("{C_KEYWORDS} {CPP_KEYWORDS}"))),
                    ("types", pred("cTypes", c_types)),
                    (
                        "blockKeywords",
                        words(&format!("{C_BLOCK_KEYWORDS} class try catch")),
                    ),
                    (
                        "defKeywords",
                        words(&format!("{C_DEF_KEYWORDS} class namespace")),
                    ),
                    ("typeFirstDefinitions", yes()),
                    ("atoms", words("true false NULL nullptr")),
                    ("dontIndentStatements", regexp("^template$", "")),
                    ("isIdentifierChar", regexp(r"[\w\$_~\xa1-\uffff]", "")),
                    ("isReservedIdentifier", reserved()),
                    ("hooks", hooks(cpp_hooks())),
                    ("namespaceSeparator", ConfigValue::Str("::".into())),
                ],
                vec![include()],
            ),
        ),
        (
            vec!["text/x-java"],
            spec(
                vec![
                    (
                        "keywords",
                        words(
                            "abstract assert break case catch class const continue default \
do else enum extends final finally for goto if implements import \
instanceof interface native new package private protected public \
return static strictfp super switch synchronized this throw throws transient \
try volatile while @interface",
                        ),
                    ),
                    (
                        "types",
                        words(
                            "var byte short int long float double boolean char void Boolean Byte Character Double Float \
Integer Long Number Object Short String StringBuffer StringBuilder Void",
                        ),
                    ),
                    (
                        "blockKeywords",
                        words("catch class do else finally for if switch try while"),
                    ),
                    ("defKeywords", words("class interface enum @interface")),
                    ("typeFirstDefinitions", yes()),
                    ("atoms", words("true false null")),
                    (
                        "number",
                        regexp(
                            r"^(?:0x[a-f\d_]+|0b[01_]+|(?:[\d_]+\.?\d*|\.\d+)(?:e[-+]?[\d_]+)?)(u|ll?|l|f)?",
                            "i",
                        ),
                    ),
                    (
                        "hooks",
                        hooks(Hooks {
                            chars: vec![
                                (u('@'), "@", java_at_hook),
                                (u('"'), "\"", java_quote_hook),
                            ],
                            ..Hooks::default()
                        }),
                    ),
                ],
                vec![fold(&["brace", "import"])],
            ),
        ),
        (
            vec!["text/x-csharp"],
            spec(
                vec![
                    (
                        "keywords",
                        words(
                            "abstract as async await base break case catch checked class const continue \
default delegate do else enum event explicit extern finally fixed for \
foreach goto if implicit in init interface internal is lock namespace new \
operator out override params private protected public readonly record ref required return sealed \
sizeof stackalloc static struct switch this throw try typeof unchecked \
unsafe using virtual void volatile while add alias ascending descending dynamic from get \
global group into join let orderby partial remove select set value var yield",
                        ),
                    ),
                    (
                        "types",
                        words(
                            "Action Boolean Byte Char DateTime DateTimeOffset Decimal Double Func \
Guid Int16 Int32 Int64 Object SByte Single String Task TimeSpan UInt16 UInt32 \
UInt64 bool byte char decimal double short int long object \
sbyte float string ushort uint ulong",
                        ),
                    ),
                    (
                        "blockKeywords",
                        words("catch class do else finally for foreach if struct switch try while"),
                    ),
                    (
                        "defKeywords",
                        words("class interface namespace record struct var"),
                    ),
                    ("typeFirstDefinitions", yes()),
                    ("atoms", words("true false null")),
                    (
                        "hooks",
                        hooks(Hooks {
                            chars: vec![(u('@'), "@", csharp_at_hook)],
                            ..Hooks::default()
                        }),
                    ),
                ],
                vec![],
            ),
        ),
        (
            vec!["text/x-scala"],
            spec(
                vec![
                    (
                        "keywords",
                        words(
                            "abstract case catch class def do else extends final finally for forSome if \
implicit import lazy match new null object override package private protected return \
sealed super this throw trait try type val var while with yield _ \
assert assume require print println printf readLine readBoolean readByte readShort \
readChar readInt readLong readFloat readDouble",
                        ),
                    ),
                    (
                        "types",
                        words(
                            "AnyVal App Application Array BufferedIterator BigDecimal BigInt Char Console Either \
Enumeration Equiv Error Exception Fractional Function IndexedSeq Int Integral Iterable \
Iterator List Map Numeric Nil NotNull Option Ordered Ordering PartialFunction PartialOrdering \
Product Proxy Range Responder Seq Serializable Set Specializable Stream StringBuilder \
StringContext Symbol Throwable Traversable TraversableOnce Tuple Unit Vector \
Boolean Byte Character CharSequence Class ClassLoader Cloneable Comparable \
Compiler Double Exception Float Integer Long Math Number Object Package Pair Process \
Runtime Runnable SecurityManager Short StackTraceElement StrictMath String \
StringBuffer System Thread ThreadGroup ThreadLocal Throwable Triple Void",
                        ),
                    ),
                    ("multiLineStrings", yes()),
                    (
                        "blockKeywords",
                        words(
                            "catch class enum do else finally for forSome if match switch try while",
                        ),
                    ),
                    (
                        "defKeywords",
                        words("class enum def object package trait type val var"),
                    ),
                    ("atoms", words("true false null")),
                    ("indentStatements", no()),
                    ("indentSwitch", no()),
                    ("isOperatorChar", regexp(r"[+\-*&%=<>!?|\/#:@]", "")),
                    (
                        "hooks",
                        hooks(Hooks {
                            chars: vec![
                                (u('@'), "@", meta_at_hook),
                                (u('"'), "\"", scala_quote_hook),
                                (u('\''), "'", scala_apos_hook),
                                (u('='), "=", scala_eq_hook),
                                (u('/'), "/", nested_comment_hook),
                            ],
                            ..Hooks::default()
                        }),
                    ),
                ],
                vec![close_brackets(Some("()[]{}\"\""), "\"")],
            ),
        ),
        (
            vec!["text/x-kotlin"],
            spec(
                vec![
                    (
                        "keywords",
                        words(
                            "package as typealias class interface this super val operator \
var fun for is in This throw return annotation \
break continue object if else while do try when !in !is as? \
file import where by get set abstract enum open inner override private public internal \
protected catch finally out final vararg reified dynamic companion constructor init \
sealed field property receiver param sparam lateinit data inline noinline tailrec \
external annotation crossinline const operator infix suspend actual expect setparam value",
                        ),
                    ),
                    (
                        "types",
                        words(
                            "Boolean Byte Character CharSequence Class ClassLoader Cloneable Comparable \
Compiler Double Exception Float Integer Long Math Number Object Package Pair Process \
Runtime Runnable SecurityManager Short StackTraceElement StrictMath String \
StringBuffer System Thread ThreadGroup ThreadLocal Throwable Triple Void Annotation Any BooleanArray \
ByteArray Char CharArray DeprecationLevel DoubleArray Enum FloatArray Function Int IntArray Lazy \
LazyThreadSafetyMode LongArray Nothing ShortArray Unit",
                        ),
                    ),
                    ("indentSwitch", no()),
                    ("indentStatements", no()),
                    ("multiLineStrings", yes()),
                    (
                        "number",
                        regexp(
                            r"^(?:0x[a-f\d_]+|0b[01_]+|(?:[\d_]+(\.\d+)?|\.\d+)(?:e[-+]?[\d_]+)?)(ul?|l|f)?",
                            "i",
                        ),
                    ),
                    (
                        "blockKeywords",
                        words("catch class do else finally for if where try while enum"),
                    ),
                    ("defKeywords", words("class val var object interface fun")),
                    ("atoms", words("true false null this")),
                    (
                        "hooks",
                        hooks(Hooks {
                            chars: vec![
                                (u('@'), "@", meta_at_hook),
                                (u('*'), "*", kotlin_star_hook),
                                (u('"'), "\"", kotlin_quote_hook),
                                (u('/'), "/", nested_comment_hook),
                            ],
                            token: None,
                            indent: Some(("indent", kotlin_indent)),
                        }),
                    ),
                ],
                vec![close_brackets(None, "\"")],
            ),
        ),
        (
            vec!["x-shader/x-vertex", "x-shader/x-fragment"],
            spec(
                vec![
                    (
                        "keywords",
                        words(
                            "sampler1D sampler2D sampler3D samplerCube \
sampler1DShadow sampler2DShadow \
const attribute uniform varying \
break continue discard return \
for while do if else struct \
in out inout",
                        ),
                    ),
                    (
                        "types",
                        words(
                            "float int bool void \
vec2 vec3 vec4 ivec2 ivec3 ivec4 bvec2 bvec3 bvec4 \
mat2 mat3 mat4",
                        ),
                    ),
                    ("blockKeywords", words("for while do if else struct")),
                    (
                        "builtin",
                        words(
                            "radians degrees sin cos tan asin acos atan \
pow exp log exp2 sqrt inversesqrt \
abs sign floor ceil fract mod min max clamp mix step smoothstep \
length distance dot cross normalize ftransform faceforward \
reflect refract matrixCompMult \
lessThan lessThanEqual greaterThan greaterThanEqual \
equal notEqual any all not \
texture1D texture1DProj texture1DLod texture1DProjLod \
texture2D texture2DProj texture2DLod texture2DProjLod \
texture3D texture3DProj texture3DLod texture3DProjLod \
textureCube textureCubeLod \
shadow1D shadow2D shadow1DProj shadow2DProj \
shadow1DLod shadow2DLod shadow1DProjLod shadow2DProjLod \
dFdx dFdy fwidth \
noise1 noise2 noise3 noise4",
                        ),
                    ),
                    (
                        "atoms",
                        words(
                            "true false \
gl_FragColor gl_SecondaryColor gl_Normal gl_Vertex \
gl_MultiTexCoord0 gl_MultiTexCoord1 gl_MultiTexCoord2 gl_MultiTexCoord3 \
gl_MultiTexCoord4 gl_MultiTexCoord5 gl_MultiTexCoord6 gl_MultiTexCoord7 \
gl_FogCoord gl_PointCoord \
gl_Position gl_PointSize gl_ClipVertex \
gl_FrontColor gl_BackColor gl_FrontSecondaryColor gl_BackSecondaryColor \
gl_TexCoord gl_FogFragCoord \
gl_FragCoord gl_FrontFacing \
gl_FragData gl_FragDepth \
gl_ModelViewMatrix gl_ProjectionMatrix gl_ModelViewProjectionMatrix \
gl_TextureMatrix gl_NormalMatrix gl_ModelViewMatrixInverse \
gl_ProjectionMatrixInverse gl_ModelViewProjectionMatrixInverse \
gl_TextureMatrixTranspose gl_ModelViewMatrixInverseTranspose \
gl_ProjectionMatrixInverseTranspose \
gl_ModelViewProjectionMatrixInverseTranspose \
gl_TextureMatrixInverseTranspose \
gl_NormalScale gl_DepthRange gl_ClipPlane \
gl_Point gl_FrontMaterial gl_BackMaterial gl_LightSource gl_LightModel \
gl_FrontLightModelProduct gl_BackLightModelProduct \
gl_TextureColor gl_EyePlaneS gl_EyePlaneT gl_EyePlaneR gl_EyePlaneQ \
gl_FogParameters \
gl_MaxLights gl_MaxClipPlanes gl_MaxTextureUnits gl_MaxTextureCoords \
gl_MaxVertexAttribs gl_MaxVertexUniformComponents gl_MaxVaryingFloats \
gl_MaxVertexTextureImageUnits gl_MaxTextureImageUnits \
gl_MaxFragmentUniformComponents gl_MaxCombineTextureImageUnits \
gl_MaxDrawBuffers",
                        ),
                    ),
                    ("indentSwitch", no()),
                    ("hooks", hooks(cpp_hash_only())),
                ],
                vec![include()],
            ),
        ),
        (
            vec!["text/x-nesc"],
            spec(
                vec![
                    (
                        "keywords",
                        words(&format!(
                            "{C_KEYWORDS} as atomic async call command component components configuration event generic \
implementation includes interface module new norace nx_struct nx_union post provides \
signal task uses abstract extends"
                        )),
                    ),
                    ("types", pred("cTypes", c_types)),
                    ("blockKeywords", words(C_BLOCK_KEYWORDS)),
                    ("atoms", words("null true false")),
                    ("hooks", hooks(cpp_hash_only())),
                ],
                vec![include()],
            ),
        ),
        (
            vec!["text/x-objectivec"],
            spec(
                vec![
                    ("keywords", words(&format!("{C_KEYWORDS} {OBJC_KEYWORDS}"))),
                    ("types", pred("objCTypes", obj_c_types)),
                    ("builtin", words(OBJC_BUILTINS)),
                    (
                        "blockKeywords",
                        words(&format!(
                            "{C_BLOCK_KEYWORDS} @synthesize @try @catch @finally @autoreleasepool @synchronized"
                        )),
                    ),
                    (
                        "defKeywords",
                        words(&format!(
                            "{C_DEF_KEYWORDS} @interface @implementation @protocol @class"
                        )),
                    ),
                    ("dontIndentStatements", regexp("^@.*$", "")),
                    ("typeFirstDefinitions", yes()),
                    ("atoms", words("YES NO NULL Nil nil true false nullptr")),
                    ("isReservedIdentifier", reserved()),
                    ("hooks", hooks(c_hooks())),
                ],
                vec![include()],
            ),
        ),
        (
            vec!["text/x-objectivec++"],
            spec(
                vec![
                    (
                        "keywords",
                        words(&format!("{C_KEYWORDS} {OBJC_KEYWORDS} {CPP_KEYWORDS}")),
                    ),
                    ("types", pred("objCTypes", obj_c_types)),
                    ("builtin", words(OBJC_BUILTINS)),
                    (
                        "blockKeywords",
                        words(&format!(
                            "{C_BLOCK_KEYWORDS} @synthesize @try @catch @finally @autoreleasepool @synchronized class try catch"
                        )),
                    ),
                    (
                        "defKeywords",
                        words(&format!(
                            "{C_DEF_KEYWORDS} @interface @implementation @protocol @class class namespace"
                        )),
                    ),
                    ("dontIndentStatements", regexp("^@.*$|^template$", "")),
                    ("typeFirstDefinitions", yes()),
                    ("atoms", words("YES NO NULL Nil nil true false nullptr")),
                    ("isReservedIdentifier", reserved()),
                    ("hooks", hooks(cpp_hooks())),
                    ("namespaceSeparator", ConfigValue::Str("::".into())),
                ],
                vec![include()],
            ),
        ),
        (
            vec!["text/x-squirrel"],
            spec(
                vec![
                    (
                        "keywords",
                        words(
                            "base break clone continue const default delete enum extends function in class \
foreach local resume return this throw typeof yield constructor instanceof static",
                        ),
                    ),
                    ("types", pred("cTypes", c_types)),
                    (
                        "blockKeywords",
                        words("case catch class else for foreach if switch try while"),
                    ),
                    ("defKeywords", words("function local class")),
                    ("typeFirstDefinitions", yes()),
                    ("atoms", words("true false null")),
                    ("hooks", hooks(cpp_hash_only())),
                ],
                vec![include()],
            ),
        ),
        (
            vec!["text/x-ceylon"],
            spec(
                vec![
                    (
                        "keywords",
                        words(
                            "abstracts alias assembly assert assign break case catch class continue dynamic else \
exists extends finally for function given if import in interface is let module new \
nonempty object of out outer package return satisfies super switch then this throw \
try value void while",
                        ),
                    ),
                    ("types", pred("types", ceylon_types)),
                    (
                        "blockKeywords",
                        words(
                            "case catch class dynamic else finally for function if interface module new object switch try while",
                        ),
                    ),
                    (
                        "defKeywords",
                        words("class dynamic function interface module object package value"),
                    ),
                    (
                        "builtin",
                        words(
                            "abstract actual aliased annotation by default deprecated doc final formal late license \
native optional sealed see serializable shared suppressWarnings tagged throws variable",
                        ),
                    ),
                    ("isPunctuationChar", regexp(r"[\[\]{}\(\),;\:\.`]", "")),
                    ("isOperatorChar", regexp(r"[+\-*&%=<>!?|^~:\/]", "")),
                    ("numberStart", regexp(r"[\d#$]", "")),
                    (
                        "number",
                        regexp(
                            r"^(?:#[\da-fA-F_]+|\$[01_]+|[\d_]+[kMGTPmunpf]?|[\d_]+\.[\d_]+(?:[eE][-+]?\d+|[kMGTPmunpf]|)|)",
                            "i",
                        ),
                    ),
                    ("multiLineStrings", yes()),
                    ("typeFirstDefinitions", yes()),
                    (
                        "atoms",
                        words("true false null larger smaller equal empty finished"),
                    ),
                    ("indentSwitch", no()),
                    ("styleDefs", no()),
                    (
                        "hooks",
                        hooks(Hooks {
                            chars: vec![
                                (u('@'), "@", meta_at_hook),
                                (u('"'), "\"", ceylon_quote_hook),
                                (u('`'), "`", ceylon_backtick_hook),
                                (u('\''), "'", ceylon_apos_hook),
                            ],
                            token: Some(("token", ceylon_token_hook)),
                            indent: None,
                        }),
                    ),
                ],
                vec![fold(&["brace", "import"]), close_brackets(None, "\"")],
            ),
        ),
    ]
}

/// `def(mimes, mode)`'s word list: the keys of `keywords`, `types`, `builtin` and
/// `atoms`, in that order (a key in two tables appears twice).
pub fn hint_words(spec: &ModeSpec) -> Vec<String> {
    let mut out = Vec::new();
    for k in ["keywords", "types", "builtin", "atoms"] {
        if let Some(w) = opt::<Words>(spec, k) {
            out.extend(w.keys().iter().cloned());
        }
    }
    out
}

/// Runs `mode/clike/clike.js`: `defineMode("clike", ...)` and every `def(...)`.
pub fn register(r: &mut Registry) {
    let env = Rc::new(Env::new());
    r.define_mode(
        "clike",
        Rc::new(move |_, config, spec| {
            Rc::new(Clike::new(env.clone(), config, spec)) as Rc<dyn Mode>
        }),
    );
    for (mimes, mut spec) in languages() {
        let words = hint_words(&spec);
        if !words.is_empty() {
            // `mode.helperType = mimes[0]; CodeMirror.registerHelper("hintWords", ...)`
            spec.helper_type = Some(mimes[0].to_string());
            r.helpers.register("hintWords", mimes[0], Rc::new(words));
        }
        for mime in &mimes {
            r.define_mime(mime, MimeTarget::Spec(spec.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Spec, copy_state, start_state};

    fn tokens(mode: &dyn Mode, state: &mut State, line: &str) -> Vec<(String, Option<String>)> {
        let text = to_u16(line);
        let mut stream = StringStream::new(&text, Some(4), None);
        let mut out = Vec::new();
        while !stream.eol() {
            let style = mode.token(&mut stream, state);
            out.push((stream.current_string(), style));
            stream.start = stream.pos;
        }
        out
    }

    fn mode(mime: &str) -> Rc<dyn Mode> {
        let mut r = Registry::new();
        register(&mut r);
        r.get_mode(&ModeOptions::default(), &Spec::from(mime))
    }

    #[test]
    fn every_mime_resolves_to_clike() {
        let mut r = Registry::new();
        register(&mut r);
        let mimes: Vec<&str> = languages().into_iter().flat_map(|(m, _)| m).collect();
        assert_eq!(mimes.len(), 16);
        for m in mimes {
            assert_eq!(
                r.get_mode(&ModeOptions::default(), &Spec::from(m)).name(),
                "clike"
            );
        }
        // Aliases share the first MIME's helperType.
        let m = r.get_mode(&ModeOptions::default(), &Spec::from("text/x-chdr"));
        assert_eq!(m.helper_type(), Some("text/x-csrc"));
    }

    #[test]
    fn reserved_identifiers_and_types() {
        let w = |s: &str| to_u16(s);
        assert!(c_is_reserved_identifier(&w("__x")));
        assert!(c_is_reserved_identifier(&w("_Complex")));
        assert!(!c_is_reserved_identifier(&w("_aName")));
        assert!(!c_is_reserved_identifier(&w("_")));
        assert!(c_types(&w("size_t")) && !c_types(&w("_t")) && c_types(&w("int")));
        assert!(ceylon_types(&w("Foo")) && !ceylon_types(&w("foo")) && !ceylon_types(&w("1")));
        // A lone surrogate has no case: not a Ceylon type.
        assert!(!ceylon_types(&[0xd801, 0xdc00]));
    }

    #[test]
    fn copied_state_shares_contexts() {
        let m = mode("text/x-csrc");
        let mut a = start_state(&*m, JsArg::Undefined, JsArg::Undefined);
        tokens(&*m, &mut a, "int f() {");
        let b = copy_state(&*m, &a);
        // The `{` context has align null until the next line starts.
        assert_eq!(a.get::<ClikeState>().context.align.get(), None);
        tokens(&*m, &mut a, "  x;");
        assert!(Rc::ptr_eq(
            &a.get::<ClikeState>().context,
            &b.get::<ClikeState>().context
        ));
        assert_eq!(b.get::<ClikeState>().context.align.get(), Some(false));
    }

    #[test]
    fn cpp_raw_string_and_preprocessor() {
        let m = mode("text/x-c++src");
        let mut st = start_state(&*m, JsArg::Undefined, JsArg::Undefined);
        let got = tokens(&*m, &mut st, r#"R"x(a)"b)x" y"#);
        assert_eq!(
            got[0],
            (r#"R"x(a)"b)x""#.to_string(), Some("string".into()))
        );
        let mut st = start_state(&*m, JsArg::Undefined, JsArg::Undefined);
        let got = tokens(&*m, &mut st, "#define A \\");
        assert_eq!(got, vec![("#define A \\".to_string(), Some("meta".into()))]);
        assert_eq!(st.get::<ClikeState>().tokenize, Some(Tokenize::CppHook));
    }
}
