//! Port of `mode/javascript/javascript.js` (CodeMirror 5.65.21): JavaScript, JSON,
//! JSON-LD and TypeScript, with every `defineMIME` of the file. Quirks are kept on
//! purpose; each function cites the JS function it ports.
//!
//! * The JS keeps `type` / `content` in closure variables of the mode instance
//!   ("scratch variables"). They survive from one token to the next, so a tokenizer
//!   path that returns without calling `ret` (a stray `\`) reuses the previous
//!   token's type. Here they are `Cell`/`RefCell` fields of [`JsMode`]; each
//!   `getMode` builds a fresh instance where both start `undefined`.
//! * `state.lexical` objects are mutated in place (`align`, `pos`) and shared by
//!   shallow state copies, so they are `Rc<RefCell<Lexical>>`.
//! * The `cc` stack holds JS functions, some of them closures (`pushlex(type, info)`,
//!   `expect(wanted)`, `commasep(...)` and its inner `proceed`, `maybeTarget`); here
//!   [`C`] enumerates them. Identity tests (`c == poplex`, `top ==
//!   maybeoperatorComma`) become variant tests.
//! * Regexps are hand-coded over UTF-16 code units with the JS source quoted.
//!
//! Not supported (documented gaps): `parserConfig.wordCharacters` (a custom
//! regexp), `parserConfig.localVars` and `parserConfig.globalVars` (objects). They
//! behave as if absent. The mode object has no `skipExpression` or
//! `expressionAllowed` property (`expressionAllowed` is internal only). The
//! `registerHelper("wordChars", "javascript", /[\w$]/)` call is not ported (there are
//! no helpers).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cm_core::text::{is_js_space, is_js_word, js_slice};
use cm_core::util::string_stream::{RegexMatch, StringStream};

use crate::js::{JsNum, JsStr, eq};
use crate::{ConfigValue, JsArg, MimeTarget, Mode, ModeOptions, ModeSpec, Registry, State};

type U = Vec<u16>;

fn u(s: &str) -> U {
    s.encode_utf16().collect()
}

fn val_is(v: &Option<U>, s: &str) -> bool {
    v.as_ref().is_some_and(|v| eq(v, s))
}

/// `/[\w$\xa1-￿]/` on one UTF-16 unit (surrogates included).
fn is_word_re(c: u16) -> bool {
    is_js_word(c) || c == b'$' as u16 || c >= 0xa1
}

/// `/[+\-*&%=<>!?|~^@]/`
fn is_operator_char(c: u16) -> bool {
    c < 0x80 && b"+-*&%=<>!?|~^@".contains(&(c as u8))
}

fn is_d(c: u16) -> bool {
    (b'0' as u16..=b'9' as u16).contains(&c)
}

fn is_d_(c: u16) -> bool {
    is_d(c) || c == b'_' as u16
}

fn at(s: &[u16], i: usize) -> Option<u16> {
    s.get(i).copied()
}

fn is_c(c: Option<u16>, ch: u8) -> bool {
    c == Some(ch as u16)
}

fn m(len: usize) -> Option<RegexMatch> {
    Some(RegexMatch { index: 0, len })
}

/// `(?:[eE][+\-]?[\d_]+)?` at `i`: the length taken (0 when absent).
fn exponent_len(s: &[u16], i: usize) -> usize {
    if !(is_c(at(s, i), b'e') || is_c(at(s, i), b'E')) {
        return 0;
    }
    let mut j = i + 1;
    if is_c(at(s, j), b'+') || is_c(at(s, j), b'-') {
        j += 1;
    }
    let k = s[j.min(s.len())..]
        .iter()
        .take_while(|&&c| is_d_(c))
        .count();
    if k > 0 { j + k - i } else { 0 }
}

/// `/^\d[\d_]*(?:[eE][+\-]?[\d_]+)?/`
fn re_dot_number(s: &[u16]) -> Option<RegexMatch> {
    if !at(s, 0).is_some_and(is_d) {
        return None;
    }
    let i = 1 + s[1..].iter().take_while(|&&c| is_d_(c)).count();
    m(i + exponent_len(s, i))
}

/// `/^(?:x[\dA-Fa-f_]+|o[0-7_]+|b[01_]+)n?/`
fn re_radix(s: &[u16]) -> Option<RegexMatch> {
    let class: fn(u16) -> bool = match at(s, 0) {
        Some(0x78) => |c| crate::js::is_hex(c) || c == b'_' as u16,
        Some(0x6f) => |c| (b'0' as u16..=b'7' as u16).contains(&c) || c == b'_' as u16,
        Some(0x62) => |c| c == b'0' as u16 || c == b'1' as u16 || c == b'_' as u16,
        _ => return None,
    };
    let k = s[1..].iter().take_while(|&&c| class(c)).count();
    if k == 0 {
        return None;
    }
    let mut i = 1 + k;
    if is_c(at(s, i), b'n') {
        i += 1;
    }
    m(i)
}

/// `/^[\d_]*(?:n|(?:\.[\d_]*)?(?:[eE][+\-]?[\d_]+)?)?/` (always matches).
fn re_decimal_rest(s: &[u16]) -> Option<RegexMatch> {
    let mut i = s.iter().take_while(|&&c| is_d_(c)).count();
    if is_c(at(s, i), b'n') {
        return m(i + 1);
    }
    if is_c(at(s, i), b'.') {
        i += 1;
        i += s[i..].iter().take_while(|&&c| is_d_(c)).count();
    }
    m(i + exponent_len(s, i))
}

/// `/^\b(([gimyus])(?![gimyus]*\2))+\b/`: the greedy run of flags that do not
/// repeat later in the run, followed by a word boundary.
fn re_regexp_flags(s: &[u16]) -> Option<RegexMatch> {
    let is_flag = |c: u16| c < 0x80 && b"gimyus".contains(&(c as u8));
    let mut k = 0;
    while k < s.len() && is_flag(s[k]) {
        let run_end = k + 1 + s[k + 1..].iter().take_while(|&&c| is_flag(c)).count();
        if s[k + 1..run_end].contains(&s[k]) {
            break;
        }
        k += 1;
    }
    if k == 0 || at(s, k).is_some_and(is_js_word) {
        return None;
    }
    m(k)
}

/// `/^(\s|\/\*([^*]|\*(?!\/))*?\*\/)*[\[\(\w]/`
fn re_async_lookahead(s: &[u16]) -> Option<RegexMatch> {
    let mut i = 0;
    loop {
        match at(s, i) {
            Some(c) if is_js_space(c) => i += 1,
            Some(0x2f) if is_c(at(s, i + 1), b'*') => {
                let mut j = i + 2;
                loop {
                    if j >= s.len() {
                        return None;
                    }
                    if s[j] == b'*' as u16 && is_c(at(s, j + 1), b'/') {
                        break;
                    }
                    j += 1;
                }
                i = j + 2;
            }
            _ => break,
        }
    }
    match at(s, i) {
        Some(c) if c == b'[' as u16 || c == b'(' as u16 || is_js_word(c) => m(i + 1),
        _ => None,
    }
}

fn spaces(s: &[u16], i: usize) -> usize {
    i + s[i.min(s.len())..]
        .iter()
        .take_while(|&&c| is_js_space(c))
        .count()
}

/// `/^\s*$/`
fn re_blank(s: &[u16]) -> Option<RegexMatch> {
    (spaces(s, 0) == s.len()).then(|| RegexMatch {
        index: 0,
        len: s.len(),
    })
}

/// `/^\s*\w/`
fn re_sp_word(s: &[u16]) -> Option<RegexMatch> {
    let i = spaces(s, 0);
    at(s, i).is_some_and(is_js_word).then(|| RegexMatch {
        index: 0,
        len: i + 1,
    })
}

/// `/^\s*:/`
fn re_sp_colon(s: &[u16]) -> Option<RegexMatch> {
    let i = spaces(s, 0);
    is_c(at(s, i), b':').then(|| RegexMatch {
        index: 0,
        len: i + 1,
    })
}

/// `/^\s*:\s*/`
fn re_sp_colon_sp(s: &[u16]) -> Option<RegexMatch> {
    let i = spaces(s, 0);
    is_c(at(s, i), b':').then(|| RegexMatch {
        index: 0,
        len: spaces(s, i + 1),
    })
}

/// `/^\s*[?:]/`
fn re_sp_q_colon(s: &[u16]) -> Option<RegexMatch> {
    let i = spaces(s, 0);
    (is_c(at(s, i), b'?') || is_c(at(s, i), b':')).then(|| RegexMatch {
        index: 0,
        len: i + 1,
    })
}

/// `/^\s*\w+\s+is\b/`
fn re_type_predicate(s: &[u16]) -> Option<RegexMatch> {
    let i = spaces(s, 0);
    let w = s[i..].iter().take_while(|&&c| is_js_word(c)).count();
    if w == 0 {
        return None;
    }
    let j = spaces(s, i + w);
    if j == i + w {
        return None;
    }
    if !(is_c(at(s, j), b'i') && is_c(at(s, j + 1), b's')) || at(s, j + 2).is_some_and(is_js_word) {
        return None;
    }
    m(j + 2)
}

/// `/^\s+#?[\w$\xa1-￿]/`
fn re_class_modifier(s: &[u16]) -> Option<RegexMatch> {
    let mut i = spaces(s, 0);
    if i == 0 {
        return None;
    }
    if is_c(at(s, i), b'#') {
        i += 1;
    }
    at(s, i).is_some_and(is_word_re).then(|| RegexMatch {
        index: 0,
        len: i + 1,
    })
}

/// `/^([^<>]|<[^<>]*>)*>\s*\(/`
fn re_ts_call_type_args(s: &[u16]) -> Option<RegexMatch> {
    let (lt, gt) = (b'<' as u16, b'>' as u16);
    let mut i = 0;
    while i < s.len() {
        if s[i] != lt && s[i] != gt {
            i += 1;
        } else if s[i] == lt {
            match s[i + 1..].iter().position(|&c| c == lt || c == gt) {
                Some(k) if s[i + 1 + k] == gt => i = i + 2 + k,
                _ => break,
            }
        } else {
            break;
        }
    }
    if at(s, i) != Some(gt) {
        return None;
    }
    let j = spaces(s, i + 1);
    is_c(at(s, j), b'(').then(|| RegexMatch {
        index: 0,
        len: j + 1,
    })
}

/// `/^@(context|id|value|language|type|container|list|set|reverse|index|base|vocab|graph)"/`
fn re_jsonld_keyword(s: &[u16]) -> Option<RegexMatch> {
    if !is_c(at(s, 0), b'@') {
        return None;
    }
    for kw in [
        "context",
        "id",
        "value",
        "language",
        "type",
        "container",
        "list",
        "set",
        "reverse",
        "index",
        "base",
        "vocab",
        "graph",
    ] {
        let n = kw.len();
        if s.len() > n + 1 && eq(&s[1..1 + n], kw) && s[1 + n] == b'"' as u16 {
            return m(n + 2);
        }
    }
    None
}

/// The TS return-type skip in `findFatArrow`:
/// `/:\s*(?:\w+(?:<[^>]*>|\[\])?|\{[^}]*\})\s*$/.exec(s)`, returning `m.index`.
fn re_ts_arrow_return(s: &[u16]) -> Option<usize> {
    let rest_blank = |i: usize| spaces(s, i) == s.len();
    (0..s.len()).find(|&p| {
        if s[p] != b':' as u16 {
            return false;
        }
        let i = spaces(s, p + 1);
        let w = s[i..].iter().take_while(|&&c| is_js_word(c)).count();
        if w > 0 {
            let j = i + w;
            if is_c(at(s, j), b'<') {
                if let Some(k) = s[j + 1..].iter().position(|&c| c == b'>' as u16) {
                    if rest_blank(j + 2 + k) {
                        return true;
                    }
                }
            }
            if is_c(at(s, j), b'[') && is_c(at(s, j + 1), b']') && rest_blank(j + 2) {
                return true;
            }
            rest_blank(j)
        } else if is_c(at(s, i), b'{') {
            match s[i + 1..].iter().position(|&c| c == b'}' as u16) {
                Some(k) => rest_blank(i + 2 + k),
                None => false,
            }
        } else {
            false
        }
    })
}

/// `/^\s*else\b/`
fn starts_else(s: &[u16]) -> bool {
    let i = spaces(s, 0);
    s.len() >= i + 4 && eq(&s[i..i + 4], "else") && !at(s, i + 4).is_some_and(is_js_word)
}

/// `/^(?:case|default)\b/`
fn starts_case_default(s: &[u16]) -> bool {
    for kw in ["case", "default"] {
        let n = kw.len();
        if s.len() >= n && eq(&s[..n], kw) && !at(s, n).is_some_and(is_js_word) {
            return true;
        }
    }
    false
}

// --------------------------------------------------------------------------------
// State

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    Base,
    Str(u16),
    Comment,
    Quasi,
}

/// `JSLexical`.
#[derive(Debug)]
pub struct Lexical {
    /// A number, or Pass when the mode started from a Pass base (see [`JsNum`]).
    indented: JsNum,
    column: f64,
    ty: &'static str,
    prev: Option<LexRef>,
    info: Option<U>,
    /// `hasOwnProperty("align")` ? value : absent.
    align: Option<bool>,
    pos: Option<f64>,
}

type LexRef = Rc<RefCell<Lexical>>;

/// `Var`.
#[derive(Debug)]
pub struct Var {
    /// `undefined` is a possible name: `#x` tokens are "variable" with no content.
    name: Option<U>,
    next: Option<Rc<Var>>,
}

/// `Context`.
#[derive(Debug)]
pub struct Ctx {
    prev: Option<Rc<Ctx>>,
    vars: Option<Rc<Var>>,
    block: bool,
}

#[derive(Debug)]
pub struct CommaSep {
    what: C,
    end: &'static str,
    sep: Option<&'static str>,
}

/// The combinators that live on the `cc` stack.
#[derive(Clone, Debug)]
enum C {
    Statement,
    Expression,
    ExpressionNoComma,
    ParenExpr,
    MaybeExpression,
    MaybeOperatorComma,
    MaybeOperatorNoComma,
    Quasi,
    ContinueQuasi,
    ArrowBody,
    ArrowBodyNoComma,
    MaybeTarget(bool),
    Target,
    TargetNoComma,
    MaybeLabel,
    Property,
    ObjProp,
    GetterSetter,
    AfterProp,
    /// The function `commasep(what, end, sep)` returns.
    Commasep(Rc<CommaSep>),
    /// Its inner `proceed`.
    Proceed(Rc<CommaSep>),
    /// The anonymous `function(type, value)` `proceed` pushes.
    CommaNext(Rc<CommaSep>),
    Block,
    MaybeType,
    MaybeTypeOrIn,
    MaybeRetType,
    IsKW,
    TypeExpr,
    MaybeReturnType,
    TypeProps,
    TypeProp,
    QuasiType,
    ContinueQuasiType,
    TypeArg,
    AfterType,
    MaybeTypeArgs,
    TypeParam,
    MaybeTypeDefault,
    VarDef,
    Pattern,
    PropPattern,
    EltPattern,
    MaybeAssign,
    VarDefCont,
    MaybeElse,
    ForSpec,
    ForSpec1,
    ForSpec2,
    FunctionDef,
    FunctionDecl,
    TypeName,
    FunArg,
    ClassExpression,
    ClassName,
    ClassNameAfter,
    ClassBody,
    ClassField,
    AfterExport,
    ExportField,
    AfterImport,
    ImportSpec,
    MaybeMoreImports,
    MaybeAs,
    MaybeFrom,
    ArrayLiteral,
    EnumDef,
    EnumMember,
    MaybeCatchBinding,
    /// `expect(wanted)`.
    Expect(&'static str),
    // `.lex = true` combinators:
    /// `pushlex(type, info)`.
    PushLex(&'static str, Option<U>),
    PopLex,
    PushContext,
    PushBlockContext,
    PopContext,
}

impl C {
    fn lex(&self) -> bool {
        matches!(
            self,
            C::PushLex(..) | C::PopLex | C::PushContext | C::PushBlockContext | C::PopContext
        )
    }
}

fn pushlex(t: &'static str) -> C {
    C::PushLex(t, None)
}

fn commasep(what: C, end: &'static str, sep: Option<&'static str>) -> C {
    C::Commasep(Rc::new(CommaSep { what, end, sep }))
}

/// The javascript mode state.
#[derive(Clone, Debug)]
pub struct JsState {
    tokenize: Tok,
    last_type: Option<&'static str>,
    cc: Vec<C>,
    lexical: LexRef,
    local_vars: Option<Rc<Var>>,
    context: Option<Rc<Ctx>>,
    indented: JsNum,
    /// `state.fatArrowAt`: absent/null is `None`.
    fat_arrow_at: Option<i64>,
}

// --------------------------------------------------------------------------------
// Mode

pub struct JsMode {
    indent_unit: f64,
    statement_indent: Option<f64>,
    jsonld_mode: bool,
    json_mode: bool,
    track_scope: bool,
    is_ts: bool,
    /// `parserConfig.doubleIndentSwitch != false`.
    double_indent_switch: bool,
    // Scratch variables `type` / `content` (javascript.js: `var type, content`).
    ty: Cell<Option<&'static str>>,
    content: RefCell<Option<U>>,
}

/// `keywords` (looked up with `propertyIsEnumerable`: own keys only).
fn keyword(w: &[u16]) -> Option<(&'static str, &'static str)> {
    const K: &[(&str, &str, &str)] = &[
        ("if", "if", "keyword"),
        ("while", "keyword a", "keyword"),
        ("with", "keyword a", "keyword"),
        ("else", "keyword b", "keyword"),
        ("do", "keyword b", "keyword"),
        ("try", "keyword b", "keyword"),
        ("finally", "keyword b", "keyword"),
        ("return", "keyword d", "keyword"),
        ("break", "keyword d", "keyword"),
        ("continue", "keyword d", "keyword"),
        ("new", "new", "keyword"),
        ("delete", "keyword c", "keyword"),
        ("void", "keyword c", "keyword"),
        ("throw", "keyword c", "keyword"),
        ("debugger", "debugger", "keyword"),
        ("var", "var", "keyword"),
        ("const", "var", "keyword"),
        ("let", "var", "keyword"),
        ("function", "function", "keyword"),
        ("catch", "catch", "keyword"),
        ("for", "for", "keyword"),
        ("switch", "switch", "keyword"),
        ("case", "case", "keyword"),
        ("default", "default", "keyword"),
        ("in", "operator", "keyword"),
        ("typeof", "operator", "keyword"),
        ("instanceof", "operator", "keyword"),
        ("true", "atom", "atom"),
        ("false", "atom", "atom"),
        ("null", "atom", "atom"),
        ("undefined", "atom", "atom"),
        ("NaN", "atom", "atom"),
        ("Infinity", "atom", "atom"),
        ("this", "this", "keyword"),
        ("class", "class", "keyword"),
        ("super", "atom", "keyword"),
        ("yield", "keyword c", "keyword"),
        ("export", "export", "keyword"),
        ("import", "import", "keyword"),
        ("extends", "keyword c", "keyword"),
        ("await", "keyword c", "keyword"),
    ];
    K.iter().find(|(k, _, _)| eq(w, k)).map(|&(_, t, s)| (t, s))
}

/// `brackets = "([{}])"` index.
fn bracket_index(c: u16) -> i64 {
    match c {
        0x28 => 0,
        0x5b => 1,
        0x7b => 2,
        0x7d => 3,
        0x5d => 4,
        0x29 => 5,
        _ => -1,
    }
}

fn is_modifier(v: &Option<U>) -> bool {
    ["public", "private", "protected", "abstract", "readonly"]
        .iter()
        .any(|k| val_is(v, k))
}

fn in_list(name: Option<&[u16]>, mut list: Option<&Rc<Var>>) -> bool {
    while let Some(v) = list {
        if v.name.as_deref() == name {
            return true;
        }
        list = v.next.as_ref();
    }
    false
}

fn truthy_num(v: Option<i64>) -> bool {
    matches!(v, Some(x) if x != 0)
}

impl JsMode {
    pub fn new(config: &ModeOptions, pc: &ModeSpec) -> JsMode {
        let jsonld_mode = pc.truthy("jsonld");
        let double_off = matches!(
            pc.get("doubleIndentSwitch"),
            Some(ConfigValue::Bool(false)) | Some(ConfigValue::Num(0.0))
        ) || matches!(pc.get("doubleIndentSwitch"), Some(ConfigValue::Str(s)) if s.is_empty() || s == "0");
        JsMode {
            indent_unit: config.indent_unit as f64,
            statement_indent: pc
                .get("statementIndent")
                .and_then(ConfigValue::as_num)
                .filter(|&x| x != 0.0 && !x.is_nan()),
            jsonld_mode,
            json_mode: pc.truthy("json") || jsonld_mode,
            track_scope: pc.get("trackScope") != Some(&ConfigValue::Bool(false)),
            is_ts: pc.truthy("typescript"),
            double_indent_switch: !double_off,
            ty: Cell::new(None),
            content: RefCell::new(None),
        }
    }

    /// javascript.js: `ret`.
    fn ret(&self, tp: &'static str, style: JsStr, cont: Option<U>) -> JsStr {
        self.ty.set(Some(tp));
        *self.content.borrow_mut() = cont;
        style
    }

    fn call_tokenize(&self, stream: &mut StringStream<'_>, state: &mut JsState) -> JsStr {
        match state.tokenize {
            Tok::Base => self.token_base(stream, state),
            Tok::Str(q) => self.token_string(q, stream, state),
            Tok::Comment => self.token_comment(stream, state),
            Tok::Quasi => self.token_quasi(stream, state),
        }
    }

    /// javascript.js: `readRegexp`.
    fn read_regexp(stream: &mut StringStream<'_>) {
        let (mut escaped, mut in_set) = (false, false);
        while let Some(next) = stream.next() {
            if !escaped {
                if next == b'/' as u16 && !in_set {
                    return;
                }
                if next == b'[' as u16 {
                    in_set = true;
                } else if in_set && next == b']' as u16 {
                    in_set = false;
                }
            }
            escaped = !escaped && next == b'\\' as u16;
        }
    }

    /// javascript.js: `tokenBase`.
    fn token_base(&self, stream: &mut StringStream<'_>, state: &mut JsState) -> JsStr {
        let ch = stream.next();
        let is = |c: u8| ch == Some(c as u16);
        if is(b'"') || is(b'\'') {
            state.tokenize = Tok::Str(ch.unwrap());
            return self.call_tokenize(stream, state);
        } else if is(b'.') && stream.match_with(re_dot_number, true).is_some() {
            return self.ret("number", "number".into(), None);
        } else if is(b'.') && stream.match_str("..", true, false) {
            return self.ret("spread", "meta".into(), None);
        } else if let Some(t) = ch.and_then(|c| match c as u8 {
            _ if c >= 0x80 => None,
            b'[' => Some("["),
            b']' => Some("]"),
            b'{' => Some("{"),
            b'}' => Some("}"),
            b'(' => Some("("),
            b')' => Some(")"),
            b',' => Some(","),
            b';' => Some(";"),
            b':' => Some(":"),
            b'.' => Some("."),
            _ => None,
        }) {
            // `/[\[\]{}\(\),;\:\.]/.test(ch)`: `ret(ch)`, style undefined.
            return self.ret(t, JsStr::Undefined, None);
        } else if is(b'=') && stream.eat(b'>' as u16).is_some() {
            return self.ret("=>", "operator".into(), None);
        } else if is(b'0') && stream.match_with(re_radix, true).is_some() {
            return self.ret("number", "number".into(), None);
        } else if ch.is_some_and(is_d) {
            stream.match_with(re_decimal_rest, true);
            return self.ret("number", "number".into(), None);
        } else if is(b'/') {
            if stream.eat(b'*' as u16).is_some() {
                state.tokenize = Tok::Comment;
                return self.token_comment(stream, state);
            } else if stream.eat(b'/' as u16).is_some() {
                stream.skip_to_end();
                return self.ret("comment", "comment".into(), None);
            } else if self.expression_allowed(stream, state, 1) {
                Self::read_regexp(stream);
                stream.match_with(re_regexp_flags, true);
                return self.ret("regexp", "string-2".into(), None);
            } else {
                stream.eat(b'=' as u16);
                let cur = stream.current().to_vec();
                return self.ret("operator", "operator".into(), Some(cur));
            }
        } else if is(b'`') {
            state.tokenize = Tok::Quasi;
            return self.token_quasi(stream, state);
        } else if is(b'#') && stream.peek() == Some(b'!' as u16) {
            stream.skip_to_end();
            return self.ret("meta", "meta".into(), None);
        } else if is(b'#') && stream.eat_while(is_word_re) {
            return self.ret("variable", "property".into(), None);
        } else if is(b'<') && stream.match_str("!--", true, false)
            || (is(b'-')
                && stream.match_str("->", true, false)
                && !crate::js::has_non_space(js_slice(stream.string, 0, Some(stream.start as i64))))
        {
            stream.skip_to_end();
            return self.ret("comment", "comment".into(), None);
        } else if ch.is_some_and(is_operator_char) {
            let c = ch.unwrap();
            let lex_gt = state.lexical.borrow().ty == ">";
            if c != b'>' as u16 || !lex_gt {
                if stream.eat(b'=' as u16).is_some() {
                    if c == b'!' as u16 || c == b'=' as u16 {
                        stream.eat(b'=' as u16);
                    }
                } else if c < 0x80 && b"<>*+-|&?".contains(&(c as u8)) {
                    stream.eat(c);
                    if c == b'>' as u16 {
                        stream.eat(c);
                    }
                }
            }
            if c == b'?' as u16 && stream.eat(b'.' as u16).is_some() {
                return self.ret(".", JsStr::Undefined, None);
            }
            let cur = stream.current().to_vec();
            return self.ret("operator", "operator".into(), Some(cur));
        } else if ch.is_none_or(is_word_re) {
            // `wordRE.test(undefined)` tests the string "undefined": true.
            stream.eat_while(is_word_re);
            let word = stream.current().to_vec();
            if state.last_type != Some(".") {
                if let Some((t, s)) = keyword(&word) {
                    return self.ret(t, s.into(), Some(word));
                }
                if eq(&word, "async") && stream.match_with(re_async_lookahead, false).is_some() {
                    return self.ret("async", "keyword".into(), Some(word));
                }
            }
            return self.ret("variable", "variable".into(), Some(word));
        }
        // Falls off the end: undefined, and `type` / `content` keep their old values.
        JsStr::Undefined
    }

    /// javascript.js: `tokenString(quote)`.
    fn token_string(
        &self,
        quote: u16,
        stream: &mut StringStream<'_>,
        state: &mut JsState,
    ) -> JsStr {
        let mut escaped = false;
        if self.jsonld_mode
            && stream.peek() == Some(b'@' as u16)
            && stream.match_with(re_jsonld_keyword, true).is_some()
        {
            state.tokenize = Tok::Base;
            return self.ret("jsonld-keyword", "meta".into(), None);
        }
        while let Some(next) = stream.next() {
            if next == quote && !escaped {
                break;
            }
            escaped = !escaped && next == b'\\' as u16;
        }
        if !escaped {
            state.tokenize = Tok::Base;
        }
        self.ret("string", "string".into(), None)
    }

    /// javascript.js: `tokenComment`.
    fn token_comment(&self, stream: &mut StringStream<'_>, state: &mut JsState) -> JsStr {
        let mut maybe_end = false;
        while let Some(ch) = stream.next() {
            if ch == b'/' as u16 && maybe_end {
                state.tokenize = Tok::Base;
                break;
            }
            maybe_end = ch == b'*' as u16;
        }
        self.ret("comment", "comment".into(), None)
    }

    /// javascript.js: `tokenQuasi`.
    fn token_quasi(&self, stream: &mut StringStream<'_>, state: &mut JsState) -> JsStr {
        let mut escaped = false;
        while let Some(next) = stream.next() {
            if !escaped
                && (next == b'`' as u16 || next == b'$' as u16 && stream.eat(b'{' as u16).is_some())
            {
                state.tokenize = Tok::Base;
                break;
            }
            escaped = !escaped && next == b'\\' as u16;
        }
        let cur = stream.current().to_vec();
        self.ret("quasi", "string-2".into(), Some(cur))
    }

    /// javascript.js: `findFatArrow`.
    fn find_fat_arrow(&self, stream: &StringStream<'_>, state: &mut JsState) {
        if truthy_num(state.fat_arrow_at) {
            state.fat_arrow_at = None;
        }
        let s = stream.string;
        let Some(mut arrow) =
            cm_core::text::index_of(s, &[b'=' as u16, b'>' as u16], stream.start).map(|a| a as i64)
        else {
            return;
        };
        if self.is_ts {
            // Try to skip TypeScript return type declarations after the arguments.
            // Quirk: `m.index` is relative to the slice, not to the line.
            if let Some(mi) = re_ts_arrow_return(js_slice(s, stream.start as i64, Some(arrow))) {
                arrow = mi as i64;
            }
        }
        let char_at = |p: i64| -> Option<u16> {
            if p < 0 {
                None
            } else {
                s.get(p as usize).copied()
            }
        };
        let (mut depth, mut saw_something) = (0, false);
        let mut pos = arrow - 1;
        while pos >= 0 {
            let ch = char_at(pos).unwrap();
            let bracket = bracket_index(ch);
            if (0..3).contains(&bracket) {
                if depth == 0 {
                    pos += 1;
                    break;
                }
                depth -= 1;
                if depth == 0 {
                    if ch == b'(' as u16 {
                        saw_something = true;
                    }
                    break;
                }
            } else if (3..6).contains(&bracket) {
                depth += 1;
            } else if is_word_re(ch) {
                saw_something = true;
            } else if ch < 0x80 && b"\"'/`".contains(&(ch as u8)) {
                loop {
                    if pos == 0 {
                        return;
                    }
                    let next = char_at(pos - 1);
                    if next == Some(ch) && char_at(pos - 2) != Some(b'\\' as u16) {
                        pos -= 1;
                        break;
                    }
                    pos -= 1;
                }
            } else if saw_something && depth == 0 {
                pos += 1;
                break;
            }
            pos -= 1;
        }
        if saw_something && depth == 0 {
            state.fat_arrow_at = Some(pos);
        }
    }

    /// javascript.js: `expressionAllowed`.
    fn expression_allowed(
        &self,
        stream: &StringStream<'_>,
        state: &JsState,
        back_up: usize,
    ) -> bool {
        const OK: &[&str] = &[
            "operator",
            "sof",
            "keyword b",
            "keyword c",
            "keyword d",
            "case",
            "new",
            "export",
            "default",
            "spread",
            "[",
            "{",
            "}",
            "(",
            ",",
            ";",
            ":",
            "=>",
        ];
        state.tokenize == Tok::Base && state.last_type.is_some_and(|t| OK.contains(&t))
            || (state.last_type == Some("quasi") && {
                // `/\{\s*$/.test(stream.string.slice(0, stream.pos - backUp))`
                let sl = js_slice(stream.string, 0, Some(stream.pos as i64 - back_up as i64));
                let mut i = sl.len();
                while i > 0 && is_js_space(sl[i - 1]) {
                    i -= 1;
                }
                i > 0 && sl[i - 1] == b'{' as u16
            })
    }

    /// javascript.js: `parseJS`.
    fn parse_js(
        &self,
        state: &mut JsState,
        style: JsStr,
        ty: Option<&'static str>,
        content: Option<U>,
        stream: &mut StringStream<'_>,
    ) -> JsStr {
        {
            let mut lex = state.lexical.borrow_mut();
            if lex.align.is_none() {
                lex.align = Some(true);
            }
        }
        let mut cx = Cx {
            m: self,
            state,
            stream,
            marked: None,
            style,
        };
        loop {
            let combinator = match cx.state.cc.pop() {
                Some(c) => c,
                None if self.json_mode => C::Expression,
                None => C::Statement,
            };
            if cx.call(combinator, ty, &content) {
                while cx.state.cc.last().is_some_and(C::lex) {
                    let c = cx.state.cc.pop().unwrap();
                    cx.call(c, None, &None);
                }
                if let Some(mk) = cx.marked.take() {
                    return JsStr::S(mk);
                }
                if ty == Some("variable") && cx.in_scope(content.as_deref()) {
                    return "variable-2".into();
                }
                return cx.style;
            }
        }
    }
}

/// javascript.js: the `cx` object the combinators share.
struct Cx<'a, 's> {
    m: &'a JsMode,
    state: &'a mut JsState,
    stream: &'a mut StringStream<'s>,
    marked: Option<String>,
    style: JsStr,
}

impl Cx<'_, '_> {
    /// javascript.js: `pass` (pushes in reverse, so the first argument runs next).
    fn pass(&mut self, cs: Vec<C>) -> bool {
        for c in cs.into_iter().rev() {
            self.state.cc.push(c);
        }
        false
    }
    /// javascript.js: `cont`.
    fn cont(&mut self, cs: Vec<C>) -> bool {
        self.pass(cs);
        true
    }
    fn mark(&mut self, s: &str) {
        self.marked = Some(s.to_string());
    }
    fn matches(&mut self, f: fn(&[u16]) -> Option<RegexMatch>) -> Option<RegexMatch> {
        self.stream.match_with(f, false)
    }
    /// javascript.js: `contCommasep(what, end, info, ...rest)`.
    fn cont_commasep(
        &mut self,
        what: C,
        end: &'static str,
        info: Option<&str>,
        rest: Vec<C>,
    ) -> bool {
        for c in rest {
            self.state.cc.push(c);
        }
        self.cont(vec![
            C::PushLex(end, info.map(u)),
            commasep(what, end, None),
            C::PopLex,
        ])
    }

    /// javascript.js: `inScope`.
    fn in_scope(&self, varname: Option<&[u16]>) -> bool {
        if !self.m.track_scope {
            return false;
        }
        if in_list(varname, self.state.local_vars.as_ref()) {
            return true;
        }
        let mut cx = self.state.context.as_ref();
        while let Some(c) = cx {
            if in_list(varname, c.vars.as_ref()) {
                return true;
            }
            cx = c.prev.as_ref();
        }
        false
    }

    /// javascript.js: `register`.
    fn register(&mut self, varname: &Option<U>) {
        self.mark("def");
        if !self.m.track_scope {
            return;
        }
        let name = varname.clone();
        if let Some(context) = self.state.context.clone() {
            let info_var = self
                .state
                .lexical
                .borrow()
                .info
                .as_deref()
                .is_some_and(|i| eq(i, "var"));
            if info_var && context.block {
                // FIXME function decls are also not block scoped
                if let Some(nc) = register_var_scoped(name.as_deref(), Some(&context)) {
                    self.state.context = Some(nc);
                    return;
                }
            } else if !in_list(name.as_deref(), self.state.local_vars.as_ref()) {
                self.state.local_vars = Some(Rc::new(Var {
                    name,
                    next: self.state.local_vars.take(),
                }));
                return;
            }
        }
        // Fall through means this is global; `parserConfig.globalVars` is not supported.
    }

    fn ty_in(ty: Option<&str>, chars: &str) -> bool {
        // `type.match(/[...]/)`; a TypeError on undefined in JS.
        let t = ty.expect("TypeError: Cannot read properties of undefined (reading 'match')");
        t.chars().any(|c| chars.contains(c))
    }

    /// Calls a combinator with `(type, value)`.
    fn call(&mut self, c: C, ty: Option<&'static str>, val: &Option<U>) -> bool {
        let is_ts = self.m.is_ts;
        let t = |s: &str| ty == Some(s);
        let v = |s: &str| val_is(val, s);
        use C::*;
        match c {
            // ---- lex combinators
            PushLex(lt, info) => {
                // javascript.js: `pushlex`
                let mut indent = self.state.indented;
                let lexical = self.state.lexical.clone();
                if lexical.borrow().ty == "stat" {
                    indent = lexical.borrow().indented;
                } else {
                    let mut outer = Some(lexical.clone());
                    while let Some(o) = outer {
                        let ob = o.borrow();
                        if !(ob.ty == ")" && ob.align == Some(true)) {
                            break;
                        }
                        indent = ob.indented;
                        outer = ob.prev.clone();
                    }
                }
                let column = self.stream.column() as f64;
                self.state.lexical = Rc::new(RefCell::new(Lexical {
                    indented: indent,
                    column,
                    ty: lt,
                    prev: Some(lexical),
                    info,
                    align: None,
                    pos: None,
                }));
                false
            }
            PopLex => {
                // javascript.js: `poplex`
                let lexical = self.state.lexical.clone();
                let lb = lexical.borrow();
                if let Some(prev) = lb.prev.clone() {
                    if lb.ty == ")" {
                        self.state.indented = lb.indented;
                    }
                    self.state.lexical = prev;
                }
                false
            }
            PushContext => {
                // javascript.js: `pushcontext`
                self.state.context = Some(Rc::new(Ctx {
                    prev: self.state.context.take(),
                    vars: self.state.local_vars.take(),
                    block: false,
                }));
                self.state.local_vars = Some(default_vars());
                false
            }
            PushBlockContext => {
                // javascript.js: `pushblockcontext`
                self.state.context = Some(Rc::new(Ctx {
                    prev: self.state.context.take(),
                    vars: self.state.local_vars.take(),
                    block: true,
                }));
                self.state.local_vars = None;
                false
            }
            PopContext => {
                // javascript.js: `popcontext`
                let c = self
                    .state
                    .context
                    .clone()
                    .expect("TypeError: popcontext without a context");
                self.state.local_vars = c.vars.clone();
                self.state.context = c.prev.clone();
                false
            }
            Expect(wanted) => {
                // javascript.js: `expect(wanted)` -> `exp`
                if t(wanted) {
                    self.cont(vec![])
                } else if wanted == ";" || t("}") || t(")") || t("]") {
                    self.pass(vec![])
                } else {
                    self.cont(vec![Expect(wanted)])
                }
            }
            Statement => self.statement(ty, val),
            Expression => self.expression_inner(ty, val, false),
            ExpressionNoComma => self.expression_inner(ty, val, true),
            ParenExpr => {
                if !t("(") {
                    return self.pass(vec![]);
                }
                self.cont(vec![pushlex(")"), MaybeExpression, Expect(")"), PopLex])
            }
            MaybeExpression => {
                if Self::ty_in(ty, ";})],") {
                    return self.pass(vec![]);
                }
                self.pass(vec![Expression])
            }
            MaybeOperatorComma => {
                if t(",") {
                    return self.cont(vec![MaybeExpression]);
                }
                self.maybe_operator_no_comma(ty, val, Some(false))
            }
            MaybeOperatorNoComma => self.maybe_operator_no_comma(ty, val, None),
            Quasi => {
                if !t("quasi") {
                    return self.pass(vec![]);
                }
                if !ends_with_dollar_brace(val) {
                    return self.cont(vec![Quasi]);
                }
                self.cont(vec![MaybeExpression, ContinueQuasi])
            }
            ContinueQuasi => {
                if t("}") {
                    self.mark("string-2");
                    self.state.tokenize = Tok::Quasi;
                    return self.cont(vec![Quasi]);
                }
                false
            }
            ArrowBody | ArrowBodyNoComma => {
                self.m.find_fat_arrow(self.stream, self.state);
                let body = if t("{") {
                    Statement
                } else if matches!(c, ArrowBody) {
                    Expression
                } else {
                    ExpressionNoComma
                };
                self.pass(vec![body])
            }
            MaybeTarget(no_comma) => {
                if t(".") {
                    self.cont(vec![if no_comma { TargetNoComma } else { Target }])
                } else if t("variable") && is_ts {
                    self.cont(vec![
                        MaybeTypeArgs,
                        if no_comma {
                            MaybeOperatorNoComma
                        } else {
                            MaybeOperatorComma
                        },
                    ])
                } else {
                    self.pass(vec![if no_comma {
                        ExpressionNoComma
                    } else {
                        Expression
                    }])
                }
            }
            Target | TargetNoComma => {
                if v("target") {
                    self.mark("keyword");
                    return self.cont(vec![if matches!(c, Target) {
                        MaybeOperatorComma
                    } else {
                        MaybeOperatorNoComma
                    }]);
                }
                false
            }
            MaybeLabel => {
                if t(":") {
                    return self.cont(vec![PopLex, Statement]);
                }
                self.pass(vec![MaybeOperatorComma, Expect(";"), PopLex])
            }
            Property => {
                if t("variable") {
                    self.mark("property");
                    return self.cont(vec![]);
                }
                false
            }
            ObjProp => self.objprop(ty, val),
            GetterSetter => {
                if !t("variable") {
                    return self.pass(vec![AfterProp]);
                }
                self.mark("property");
                self.cont(vec![FunctionDef])
            }
            AfterProp => {
                if t(":") {
                    return self.cont(vec![ExpressionNoComma]);
                }
                if t("(") {
                    return self.pass(vec![FunctionDef]);
                }
                false
            }
            Commasep(cs) => {
                if t(cs.end) || v(cs.end) {
                    return self.cont(vec![]);
                }
                let what = cs.what.clone();
                self.pass(vec![what, Proceed(cs)])
            }
            Proceed(cs) => {
                let hit = match cs.sep {
                    Some(sep) => ty.is_some_and(|t| sep.contains(t)),
                    None => t(","),
                };
                if hit {
                    let lex = self.state.lexical.clone();
                    let mut lb = lex.borrow_mut();
                    if lb.info.as_deref().is_some_and(|i| eq(i, "call")) {
                        lb.pos = Some(
                            match lb.pos {
                                Some(p) if p != 0.0 && !p.is_nan() => p,
                                _ => 0.0,
                            } + 1.0,
                        );
                    }
                    drop(lb);
                    return self.cont(vec![CommaNext(cs.clone()), Proceed(cs)]);
                }
                if t(cs.end) || v(cs.end) {
                    return self.cont(vec![]);
                }
                if cs.sep.is_some_and(|s| s.contains(';')) {
                    return self.pass(vec![cs.what.clone()]);
                }
                self.cont(vec![Expect(cs.end)])
            }
            CommaNext(cs) => {
                if t(cs.end) || v(cs.end) {
                    return self.pass(vec![]);
                }
                self.pass(vec![cs.what.clone()])
            }
            Block => {
                if t("}") {
                    return self.cont(vec![]);
                }
                self.pass(vec![Statement, Block])
            }
            MaybeType => {
                if is_ts {
                    if t(":") {
                        return self.cont(vec![TypeExpr]);
                    }
                    if v("?") {
                        return self.cont(vec![MaybeType]);
                    }
                }
                false
            }
            MaybeTypeOrIn => {
                if is_ts && (t(":") || v("in")) {
                    return self.cont(vec![TypeExpr]);
                }
                false
            }
            MaybeRetType => {
                if is_ts && t(":") {
                    if self.matches(re_type_predicate).is_some() {
                        return self.cont(vec![Expression, IsKW, TypeExpr]);
                    }
                    return self.cont(vec![TypeExpr]);
                }
                false
            }
            IsKW => {
                if v("is") {
                    self.mark("keyword");
                    return self.cont(vec![]);
                }
                false
            }
            TypeExpr => self.typeexpr(ty, val),
            MaybeReturnType => {
                if t("=>") {
                    return self.cont(vec![TypeExpr]);
                }
                false
            }
            TypeProps => {
                if Self::ty_in(ty, "})]") {
                    return self.cont(vec![]);
                }
                if t(",") || t(";") {
                    return self.cont(vec![TypeProps]);
                }
                self.pass(vec![TypeProp, TypeProps])
            }
            TypeProp => {
                if t("variable") || self.style.is("keyword") {
                    self.mark("property");
                    self.cont(vec![TypeProp])
                } else if v("?") || t("number") || t("string") {
                    self.cont(vec![TypeProp])
                } else if t(":") {
                    self.cont(vec![TypeExpr])
                } else if t("[") {
                    self.cont(vec![
                        Expect("variable"),
                        MaybeTypeOrIn,
                        Expect("]"),
                        TypeProp,
                    ])
                } else if t("(") {
                    self.pass(vec![FunctionDecl, TypeProp])
                } else if !Self::ty_in(ty, ";})],") {
                    self.cont(vec![])
                } else {
                    false
                }
            }
            QuasiType => {
                if !t("quasi") {
                    return self.pass(vec![]);
                }
                if !ends_with_dollar_brace(val) {
                    return self.cont(vec![QuasiType]);
                }
                self.cont(vec![TypeExpr, ContinueQuasiType])
            }
            ContinueQuasiType => {
                if t("}") {
                    self.mark("string-2");
                    self.state.tokenize = Tok::Quasi;
                    return self.cont(vec![QuasiType]);
                }
                false
            }
            TypeArg => {
                if t("variable") && self.matches(re_sp_q_colon).is_some() || v("?") {
                    return self.cont(vec![TypeArg]);
                }
                if t(":") {
                    return self.cont(vec![TypeExpr]);
                }
                if t("spread") {
                    return self.cont(vec![TypeArg]);
                }
                self.pass(vec![TypeExpr])
            }
            AfterType => {
                if v("<") {
                    return self.cont(vec![
                        pushlex(">"),
                        commasep(TypeExpr, ">", None),
                        PopLex,
                        AfterType,
                    ]);
                }
                if v("|") || t(".") || v("&") {
                    return self.cont(vec![TypeExpr]);
                }
                if t("[") {
                    return self.cont(vec![TypeExpr, Expect("]"), AfterType]);
                }
                if v("extends") || v("implements") {
                    self.mark("keyword");
                    return self.cont(vec![TypeExpr]);
                }
                if v("?") {
                    return self.cont(vec![TypeExpr, Expect(":"), TypeExpr]);
                }
                false
            }
            MaybeTypeArgs => {
                if v("<") {
                    return self.cont(vec![
                        pushlex(">"),
                        commasep(TypeExpr, ">", None),
                        PopLex,
                        AfterType,
                    ]);
                }
                false
            }
            TypeParam => self.pass(vec![TypeExpr, MaybeTypeDefault]),
            MaybeTypeDefault => {
                if v("=") {
                    return self.cont(vec![TypeExpr]);
                }
                false
            }
            VarDef => {
                if v("enum") {
                    self.mark("keyword");
                    return self.cont(vec![EnumDef]);
                }
                self.pass(vec![Pattern, MaybeType, MaybeAssign, VarDefCont])
            }
            Pattern => {
                if is_ts && is_modifier(val) {
                    self.mark("keyword");
                    return self.cont(vec![Pattern]);
                }
                if t("variable") {
                    self.register(val);
                    return self.cont(vec![]);
                }
                if t("spread") {
                    return self.cont(vec![Pattern]);
                }
                if t("[") {
                    return self.cont_commasep(EltPattern, "]", None, vec![]);
                }
                if t("{") {
                    return self.cont_commasep(PropPattern, "}", None, vec![]);
                }
                false
            }
            PropPattern => {
                if t("variable") && self.matches(re_sp_colon).is_none() {
                    self.register(val);
                    return self.cont(vec![MaybeAssign]);
                }
                if t("variable") {
                    self.mark("property");
                }
                if t("spread") {
                    return self.cont(vec![Pattern]);
                }
                if t("}") {
                    return self.pass(vec![]);
                }
                if t("[") {
                    return self.cont(vec![Expression, Expect("]"), Expect(":"), PropPattern]);
                }
                self.cont(vec![Expect(":"), Pattern, MaybeAssign])
            }
            EltPattern => self.pass(vec![Pattern, MaybeAssign]),
            MaybeAssign => {
                if v("=") {
                    return self.cont(vec![ExpressionNoComma]);
                }
                false
            }
            VarDefCont => {
                if t(",") {
                    return self.cont(vec![VarDef]);
                }
                false
            }
            MaybeElse => {
                if t("keyword b") && v("else") {
                    return self.cont(vec![C::PushLex("form", Some(u("else"))), Statement, PopLex]);
                }
                false
            }
            ForSpec => {
                if v("await") {
                    return self.cont(vec![ForSpec]);
                }
                if t("(") {
                    return self.cont(vec![pushlex(")"), ForSpec1, PopLex]);
                }
                false
            }
            ForSpec1 => {
                if t("var") {
                    return self.cont(vec![VarDef, ForSpec2]);
                }
                if t("variable") {
                    return self.cont(vec![ForSpec2]);
                }
                self.pass(vec![ForSpec2])
            }
            ForSpec2 => {
                if t(")") {
                    return self.cont(vec![]);
                }
                if t(";") {
                    return self.cont(vec![ForSpec2]);
                }
                if v("in") || v("of") {
                    self.mark("keyword");
                    return self.cont(vec![Expression, ForSpec2]);
                }
                self.pass(vec![Expression, ForSpec2])
            }
            FunctionDef | FunctionDecl => {
                let me = c.clone();
                if v("*") {
                    self.mark("keyword");
                    return self.cont(vec![me]);
                }
                if t("variable") {
                    self.register(val);
                    return self.cont(vec![me]);
                }
                if t("(") {
                    return if matches!(c, FunctionDef) {
                        self.cont(vec![
                            PushContext,
                            pushlex(")"),
                            commasep(FunArg, ")", None),
                            PopLex,
                            MaybeRetType,
                            Statement,
                            PopContext,
                        ])
                    } else {
                        self.cont(vec![
                            PushContext,
                            pushlex(")"),
                            commasep(FunArg, ")", None),
                            PopLex,
                            MaybeRetType,
                            PopContext,
                        ])
                    };
                }
                if is_ts && v("<") {
                    return self.cont(vec![
                        pushlex(">"),
                        commasep(TypeParam, ">", None),
                        PopLex,
                        me,
                    ]);
                }
                false
            }
            TypeName => {
                if t("keyword") || t("variable") {
                    self.mark("type");
                    return self.cont(vec![TypeName]);
                } else if v("<") {
                    return self.cont(vec![pushlex(">"), commasep(TypeParam, ">", None), PopLex]);
                }
                false
            }
            FunArg => {
                if v("@") {
                    // `cont(expression, funarg)` without `return`: pushes and goes on.
                    self.cont(vec![Expression, FunArg]);
                }
                if t("spread") {
                    return self.cont(vec![FunArg]);
                }
                if is_ts && is_modifier(val) {
                    self.mark("keyword");
                    return self.cont(vec![FunArg]);
                }
                if is_ts && t("this") {
                    return self.cont(vec![MaybeType, MaybeAssign]);
                }
                self.pass(vec![Pattern, MaybeType, MaybeAssign])
            }
            ClassExpression => {
                // Class expressions may have an optional name.
                if t("variable") {
                    return self.call(ClassName, ty, val);
                }
                self.call(ClassNameAfter, ty, val)
            }
            ClassName => {
                if t("variable") {
                    self.register(val);
                    return self.cont(vec![ClassNameAfter]);
                }
                false
            }
            ClassNameAfter => {
                if v("<") {
                    return self.cont(vec![
                        pushlex(">"),
                        commasep(TypeParam, ">", None),
                        PopLex,
                        ClassNameAfter,
                    ]);
                }
                if v("extends") || v("implements") || (is_ts && t(",")) {
                    if v("implements") {
                        self.mark("keyword");
                    }
                    return self.cont(vec![
                        if is_ts { TypeExpr } else { Expression },
                        ClassNameAfter,
                    ]);
                }
                if t("{") {
                    return self.cont(vec![pushlex("}"), ClassBody, PopLex]);
                }
                false
            }
            ClassBody => {
                if t("async")
                    || (t("variable")
                        && (v("static") || v("get") || v("set") || (is_ts && is_modifier(val)))
                        && self.matches(re_class_modifier).is_some())
                {
                    self.mark("keyword");
                    return self.cont(vec![ClassBody]);
                }
                if t("variable") || self.style.is("keyword") {
                    self.mark("property");
                    return self.cont(vec![ClassField, ClassBody]);
                }
                if t("number") || t("string") {
                    return self.cont(vec![ClassField, ClassBody]);
                }
                if t("[") {
                    return self.cont(vec![
                        Expression,
                        MaybeType,
                        Expect("]"),
                        ClassField,
                        ClassBody,
                    ]);
                }
                if v("*") {
                    self.mark("keyword");
                    return self.cont(vec![ClassBody]);
                }
                if is_ts && t("(") {
                    return self.pass(vec![FunctionDecl, ClassBody]);
                }
                if t(";") || t(",") {
                    return self.cont(vec![ClassBody]);
                }
                if t("}") {
                    return self.cont(vec![]);
                }
                if v("@") {
                    return self.cont(vec![Expression, ClassBody]);
                }
                false
            }
            ClassField => {
                if v("!") || v("?") {
                    return self.cont(vec![ClassField]);
                }
                if t(":") {
                    return self.cont(vec![TypeExpr, MaybeAssign]);
                }
                if v("=") {
                    return self.cont(vec![ExpressionNoComma]);
                }
                let is_interface = self.state.lexical.borrow().prev.as_ref().is_some_and(|p| {
                    p.borrow()
                        .info
                        .as_deref()
                        .is_some_and(|i| eq(i, "interface"))
                });
                self.pass(vec![if is_interface {
                    FunctionDecl
                } else {
                    FunctionDef
                }])
            }
            AfterExport => {
                if v("*") {
                    self.mark("keyword");
                    return self.cont(vec![MaybeFrom, Expect(";")]);
                }
                if v("default") {
                    self.mark("keyword");
                    return self.cont(vec![Expression, Expect(";")]);
                }
                if t("{") {
                    return self.cont(vec![
                        commasep(ExportField, "}", None),
                        MaybeFrom,
                        Expect(";"),
                    ]);
                }
                self.pass(vec![Statement])
            }
            ExportField => {
                if v("as") {
                    self.mark("keyword");
                    return self.cont(vec![Expect("variable")]);
                }
                if t("variable") {
                    return self.pass(vec![ExpressionNoComma, ExportField]);
                }
                false
            }
            AfterImport => {
                if t("string") {
                    return self.cont(vec![]);
                }
                if t("(") {
                    return self.pass(vec![Expression]);
                }
                if t(".") {
                    return self.pass(vec![MaybeOperatorComma]);
                }
                self.pass(vec![ImportSpec, MaybeMoreImports, MaybeFrom])
            }
            ImportSpec => {
                if t("{") {
                    return self.cont_commasep(ImportSpec, "}", None, vec![]);
                }
                if t("variable") {
                    self.register(val);
                }
                if v("*") {
                    self.mark("keyword");
                }
                self.cont(vec![MaybeAs])
            }
            MaybeMoreImports => {
                if t(",") {
                    return self.cont(vec![ImportSpec, MaybeMoreImports]);
                }
                false
            }
            MaybeAs => {
                if v("as") {
                    self.mark("keyword");
                    return self.cont(vec![ImportSpec]);
                }
                false
            }
            MaybeFrom => {
                if v("from") {
                    self.mark("keyword");
                    return self.cont(vec![Expression]);
                }
                false
            }
            ArrayLiteral => {
                if t("]") {
                    return self.cont(vec![]);
                }
                self.pass(vec![commasep(ExpressionNoComma, "]", None)])
            }
            EnumDef => self.pass(vec![
                pushlex("form"),
                Pattern,
                Expect("{"),
                pushlex("}"),
                commasep(EnumMember, "}", None),
                PopLex,
                PopLex,
            ]),
            EnumMember => self.pass(vec![Pattern, MaybeAssign]),
            MaybeCatchBinding => {
                if t("(") {
                    return self.cont(vec![FunArg, Expect(")")]);
                }
                false
            }
        }
    }

    /// javascript.js: `statement`.
    fn statement(&mut self, ty: Option<&'static str>, val: &Option<U>) -> bool {
        use C::*;
        let is_ts = self.m.is_ts;
        let t = |s: &str| ty == Some(s);
        let v = |s: &str| val_is(val, s);
        if t("var") {
            return self.cont(vec![
                C::PushLex("vardef", val.clone()),
                VarDef,
                Expect(";"),
                PopLex,
            ]);
        }
        if t("keyword a") {
            return self.cont(vec![pushlex("form"), ParenExpr, Statement, PopLex]);
        }
        if t("keyword b") {
            return self.cont(vec![pushlex("form"), Statement, PopLex]);
        }
        if t("keyword d") {
            return if self.matches(re_blank).is_some() {
                self.cont(vec![])
            } else {
                self.cont(vec![pushlex("stat"), MaybeExpression, Expect(";"), PopLex])
            };
        }
        if t("debugger") {
            return self.cont(vec![Expect(";")]);
        }
        if t("{") {
            return self.cont(vec![
                pushlex("}"),
                PushBlockContext,
                Block,
                PopLex,
                PopContext,
            ]);
        }
        if t(";") {
            return self.cont(vec![]);
        }
        if t("if") {
            let info_else = self
                .state
                .lexical
                .borrow()
                .info
                .as_deref()
                .is_some_and(|i| eq(i, "else"));
            if info_else && matches!(self.state.cc.last(), Some(PopLex)) {
                let c = self.state.cc.pop().unwrap();
                self.call(c, None, &None);
            }
            return self.cont(vec![
                pushlex("form"),
                ParenExpr,
                Statement,
                PopLex,
                MaybeElse,
            ]);
        }
        if t("function") {
            return self.cont(vec![FunctionDef]);
        }
        if t("for") {
            return self.cont(vec![
                pushlex("form"),
                PushBlockContext,
                ForSpec,
                Statement,
                PopContext,
                PopLex,
            ]);
        }
        if t("class") || (is_ts && v("interface")) {
            self.mark("keyword");
            let info = if t("class") {
                Some(u("class"))
            } else {
                val.clone()
            };
            return self.cont(vec![C::PushLex("form", info), ClassName, PopLex]);
        }
        if t("variable") {
            if is_ts && v("declare") {
                self.mark("keyword");
                return self.cont(vec![Statement]);
            } else if is_ts
                && (v("module") || v("enum") || v("type"))
                && self.matches(re_sp_word).is_some()
            {
                self.mark("keyword");
                if v("enum") {
                    return self.cont(vec![EnumDef]);
                } else if v("type") {
                    return self.cont(vec![TypeName, Expect("operator"), TypeExpr, Expect(";")]);
                } else {
                    return self.cont(vec![
                        pushlex("form"),
                        Pattern,
                        Expect("{"),
                        pushlex("}"),
                        Block,
                        PopLex,
                        PopLex,
                    ]);
                }
            } else if is_ts && v("namespace") {
                self.mark("keyword");
                return self.cont(vec![pushlex("form"), Expression, Statement, PopLex]);
            } else if is_ts && v("abstract") {
                self.mark("keyword");
                return self.cont(vec![Statement]);
            } else {
                return self.cont(vec![pushlex("stat"), MaybeLabel]);
            }
        }
        if t("switch") {
            return self.cont(vec![
                pushlex("form"),
                ParenExpr,
                Expect("{"),
                C::PushLex("}", Some(u("switch"))),
                PushBlockContext,
                Block,
                PopLex,
                PopLex,
                PopContext,
            ]);
        }
        if t("case") {
            return self.cont(vec![Expression, Expect(":")]);
        }
        if t("default") {
            return self.cont(vec![Expect(":")]);
        }
        if t("catch") {
            return self.cont(vec![
                pushlex("form"),
                PushContext,
                MaybeCatchBinding,
                Statement,
                PopLex,
                PopContext,
            ]);
        }
        if t("export") {
            return self.cont(vec![pushlex("stat"), AfterExport, PopLex]);
        }
        if t("import") {
            return self.cont(vec![pushlex("stat"), AfterImport, PopLex]);
        }
        if t("async") {
            return self.cont(vec![Statement]);
        }
        if v("@") {
            return self.cont(vec![Expression, Statement]);
        }
        self.pass(vec![pushlex("stat"), Expression, Expect(";"), PopLex])
    }

    /// javascript.js: `expressionInner`.
    fn expression_inner(
        &mut self,
        ty: Option<&'static str>,
        val: &Option<U>,
        no_comma: bool,
    ) -> bool {
        use C::*;
        let is_ts = self.m.is_ts;
        let t = |s: &str| ty == Some(s);
        if self.state.fat_arrow_at == Some(self.stream.start as i64) {
            let body = if no_comma {
                ArrowBodyNoComma
            } else {
                ArrowBody
            };
            if t("(") {
                return self.cont(vec![
                    PushContext,
                    pushlex(")"),
                    commasep(FunArg, ")", None),
                    PopLex,
                    Expect("=>"),
                    body,
                    PopContext,
                ]);
            } else if t("variable") {
                return self.pass(vec![PushContext, Pattern, Expect("=>"), body, PopContext]);
            }
        }
        let maybeop = if no_comma {
            MaybeOperatorNoComma
        } else {
            MaybeOperatorComma
        };
        const ATOMIC: &[&str] = &[
            "atom",
            "number",
            "variable",
            "string",
            "regexp",
            "this",
            "import",
            "jsonld-keyword",
        ];
        if ty.is_some_and(|t| ATOMIC.contains(&t)) {
            return self.cont(vec![maybeop]);
        }
        if t("function") {
            return self.cont(vec![FunctionDef, maybeop]);
        }
        if t("class") || (is_ts && val_is(val, "interface")) {
            self.mark("keyword");
            return self.cont(vec![pushlex("form"), ClassExpression, PopLex]);
        }
        if t("keyword c") || t("async") {
            return self.cont(vec![if no_comma {
                ExpressionNoComma
            } else {
                Expression
            }]);
        }
        if t("(") {
            return self.cont(vec![
                pushlex(")"),
                MaybeExpression,
                Expect(")"),
                PopLex,
                maybeop,
            ]);
        }
        if t("operator") || t("spread") {
            return self.cont(vec![if no_comma {
                ExpressionNoComma
            } else {
                Expression
            }]);
        }
        if t("[") {
            return self.cont(vec![pushlex("]"), ArrayLiteral, PopLex, maybeop]);
        }
        if t("{") {
            return self.cont_commasep(ObjProp, "}", None, vec![maybeop]);
        }
        if t("quasi") {
            return self.pass(vec![Quasi, maybeop]);
        }
        if t("new") {
            return self.cont(vec![MaybeTarget(no_comma)]);
        }
        self.cont(vec![])
    }

    /// javascript.js: `maybeoperatorNoComma(type, value, noComma)`. `noComma` is
    /// `false` when reached from `maybeoperatorComma` and undefined when the
    /// combinator runs on its own (then `noComma == false` is false, and
    /// `noComma ? arrowBodyNoComma : arrowBody` picks `arrowBody`).
    fn maybe_operator_no_comma(
        &mut self,
        ty: Option<&'static str>,
        val: &Option<U>,
        no_comma: Option<bool>,
    ) -> bool {
        use C::*;
        let is_ts = self.m.is_ts;
        let t = |s: &str| ty == Some(s);
        let v = |s: &str| val_is(val, s);
        let me = if no_comma == Some(false) {
            MaybeOperatorComma
        } else {
            MaybeOperatorNoComma
        };
        let expr = if no_comma == Some(false) {
            Expression
        } else {
            ExpressionNoComma
        };
        if t("=>") {
            return self.cont(vec![
                PushContext,
                if no_comma == Some(true) {
                    ArrowBodyNoComma
                } else {
                    ArrowBody
                },
                PopContext,
            ]);
        }
        if t("operator") {
            // `/\+\+|--/.test(value)`
            let incdec = val
                .as_ref()
                .is_some_and(|s| s.windows(2).any(|w| w == [0x2b, 0x2b] || w == [0x2d, 0x2d]));
            if incdec || is_ts && v("!") {
                return self.cont(vec![me]);
            }
            if is_ts && v("<") && self.matches(re_ts_call_type_args).is_some() {
                return self.cont(vec![
                    pushlex(">"),
                    commasep(TypeExpr, ">", None),
                    PopLex,
                    me,
                ]);
            }
            if v("?") {
                return self.cont(vec![Expression, Expect(":"), expr]);
            }
            return self.cont(vec![expr]);
        }
        if t("quasi") {
            return self.pass(vec![Quasi, me]);
        }
        if t(";") {
            return false;
        }
        if t("(") {
            return self.cont_commasep(ExpressionNoComma, ")", Some("call"), vec![me]);
        }
        if t(".") {
            return self.cont(vec![Property, me]);
        }
        if t("[") {
            return self.cont(vec![pushlex("]"), MaybeExpression, Expect("]"), PopLex, me]);
        }
        if is_ts && v("as") {
            self.mark("keyword");
            return self.cont(vec![TypeExpr, me]);
        }
        if t("regexp") {
            self.state.last_type = Some("operator");
            self.mark("operator");
            let n = self.stream.pos - self.stream.start - 1;
            self.stream.back_up(n);
            return self.cont(vec![expr]);
        }
        false
    }

    /// javascript.js: `objprop`.
    fn objprop(&mut self, ty: Option<&'static str>, val: &Option<U>) -> bool {
        use C::*;
        let is_ts = self.m.is_ts;
        let t = |s: &str| ty == Some(s);
        let v = |s: &str| val_is(val, s);
        if t("async") {
            self.mark("property");
            self.cont(vec![ObjProp])
        } else if t("variable") || self.style.is("keyword") {
            self.mark("property");
            if v("get") || v("set") {
                return self.cont(vec![GetterSetter]);
            }
            // Work around fat-arrow-detection complication for detecting typescript typed arrow params
            if is_ts && self.state.fat_arrow_at == Some(self.stream.start as i64) {
                if let Some(mm) = self.matches(re_sp_colon_sp) {
                    self.state.fat_arrow_at = Some((self.stream.pos + mm.len) as i64);
                }
            }
            self.cont(vec![AfterProp])
        } else if t("number") || t("string") {
            self.marked = Some(if self.m.jsonld_mode {
                "property".to_string()
            } else {
                self.style.concat(" property").as_js_string()
            });
            self.cont(vec![AfterProp])
        } else if t("jsonld-keyword") {
            self.cont(vec![AfterProp])
        } else if is_ts && is_modifier(val) {
            self.mark("keyword");
            self.cont(vec![ObjProp])
        } else if t("[") {
            self.cont(vec![Expression, MaybeType, Expect("]"), AfterProp])
        } else if t("spread") {
            self.cont(vec![ExpressionNoComma, AfterProp])
        } else if v("*") {
            self.mark("keyword");
            self.cont(vec![ObjProp])
        } else if t(":") {
            self.pass(vec![AfterProp])
        } else {
            false
        }
    }

    /// javascript.js: `typeexpr`.
    fn typeexpr(&mut self, ty: Option<&'static str>, val: &Option<U>) -> bool {
        use C::*;
        let t = |s: &str| ty == Some(s);
        let v = |s: &str| val_is(val, s);
        if v("keyof") || v("typeof") || v("infer") || v("readonly") {
            self.mark("keyword");
            return self.cont(vec![if v("typeof") {
                ExpressionNoComma
            } else {
                TypeExpr
            }]);
        }
        if t("variable") || v("void") {
            self.mark("type");
            return self.cont(vec![AfterType]);
        }
        if v("|") || v("&") {
            return self.cont(vec![TypeExpr]);
        }
        if t("string") || t("number") || t("atom") {
            return self.cont(vec![AfterType]);
        }
        if t("[") {
            return self.cont(vec![
                pushlex("]"),
                commasep(TypeExpr, "]", Some(",")),
                PopLex,
                AfterType,
            ]);
        }
        if t("{") {
            return self.cont(vec![pushlex("}"), TypeProps, PopLex, AfterType]);
        }
        if t("(") {
            return self.cont(vec![
                commasep(TypeArg, ")", None),
                MaybeReturnType,
                AfterType,
            ]);
        }
        if t("<") {
            return self.cont(vec![commasep(TypeExpr, ">", None), TypeExpr]);
        }
        if t("quasi") {
            return self.pass(vec![QuasiType, AfterType]);
        }
        false
    }
}

/// `value.slice(value.length - 2) == "${"`.
fn ends_with_dollar_brace(val: &Option<U>) -> bool {
    let v = val.as_ref().expect("TypeError: quasi without content");
    v.len() >= 2 && v[v.len() - 2..] == [b'$' as u16, b'{' as u16]
}

/// javascript.js: `defaultVars = new Var("this", new Var("arguments", null))`.
fn default_vars() -> Rc<Var> {
    Rc::new(Var {
        name: Some(u("this")),
        next: Some(Rc::new(Var {
            name: Some(u("arguments")),
            next: None,
        })),
    })
}

/// javascript.js: `registerVarScoped`.
fn register_var_scoped(varname: Option<&[u16]>, context: Option<&Rc<Ctx>>) -> Option<Rc<Ctx>> {
    let context = context?;
    if context.block {
        let inner = register_var_scoped(varname, context.prev.as_ref())?;
        if context.prev.as_ref().is_some_and(|p| Rc::ptr_eq(p, &inner)) {
            return Some(context.clone());
        }
        Some(Rc::new(Ctx {
            prev: Some(inner),
            vars: context.vars.clone(),
            block: true,
        }))
    } else if in_list(varname, context.vars.as_ref()) {
        Some(context.clone())
    } else {
        Some(Rc::new(Ctx {
            prev: context.prev.clone(),
            vars: Some(Rc::new(Var {
                name: varname.map(<[u16]>::to_vec),
                next: context.vars.clone(),
            })),
            block: false,
        }))
    }
}

impl Mode for JsMode {
    fn name(&self) -> &str {
        "javascript"
    }

    /// javascript.js: `startState(basecolumn)`.
    fn start_state(&self, basecolumn: JsArg, _a2: JsArg) -> Option<State> {
        // `basecolumn || 0`: Pass stays Pass; `Pass - indentUnit` is NaN.
        let base = JsNum::or_zero(basecolumn);
        Some(State::new(JsState {
            tokenize: Tok::Base,
            last_type: Some("sof"),
            cc: Vec::new(),
            lexical: Rc::new(RefCell::new(Lexical {
                indented: JsNum::Num(base.minus(self.indent_unit)),
                column: 0.0,
                ty: "block",
                prev: None,
                info: None,
                align: Some(false),
                pos: None,
            })),
            local_vars: None,
            context: None,
            indented: base,
            fat_arrow_at: None,
        }))
    }

    /// javascript.js: `token`.
    fn token(&self, stream: &mut StringStream<'_>, state: &mut State) -> Option<String> {
        let state = state.get_mut::<JsState>();
        if stream.sol() {
            {
                let mut lex = state.lexical.borrow_mut();
                if lex.align.is_none() {
                    lex.align = Some(false);
                }
            }
            state.indented = JsNum::Num(stream.indentation() as f64);
            self.find_fat_arrow(stream, state);
        }
        if state.tokenize != Tok::Comment && stream.eat_space() {
            return None;
        }
        let style = self.call_tokenize(stream, state);
        let ty = self.ty.get();
        if ty == Some("comment") {
            return style.into_style();
        }
        let content = self.content.borrow().clone();
        let incdec = content
            .as_deref()
            .is_some_and(|c| eq(c, "++") || eq(c, "--"));
        state.last_type = if ty == Some("operator") && incdec {
            Some("incdec")
        } else {
            ty
        };
        self.parse_js(state, style, ty, content, stream)
            .into_style()
    }

    fn has_indent(&self) -> bool {
        true
    }

    /// javascript.js: `indent`.
    fn indent(&self, state: &State, text_after: &[u16], _line: &[u16]) -> Option<f64> {
        let state = state.get::<JsState>();
        if state.tokenize == Tok::Comment || state.tokenize == Tok::Quasi {
            return None;
        }
        if state.tokenize != Tok::Base {
            return Some(0.0);
        }
        let first_char = text_after.first().copied();
        let mut lexical = state.lexical.clone();
        let prev_of = |l: &LexRef| {
            l.borrow()
                .prev
                .clone()
                .expect("TypeError: lexical.prev is null")
        };
        // Kludge to prevent 'maybelse' from blocking lexical scope pops
        if !starts_else(text_after) {
            for c in state.cc.iter().rev() {
                match c {
                    C::PopLex => lexical = prev_of(&lexical),
                    C::MaybeElse | C::PopContext => {}
                    _ => break,
                }
            }
        }
        loop {
            let lt = lexical.borrow().ty;
            if !(lt == "stat" || lt == "form") {
                break;
            }
            let cont = first_char == Some(b'}' as u16)
                || (matches!(
                    state.cc.last(),
                    Some(C::MaybeOperatorComma | C::MaybeOperatorNoComma)
                ) && !first_char
                    .is_some_and(|c| c < 0x80 && b",.=+-*:?[(".contains(&(c as u8))));
            if !cont {
                break;
            }
            lexical = prev_of(&lexical);
        }
        if self.statement_indent.is_some()
            && lexical.borrow().ty == ")"
            && prev_of(&lexical).borrow().ty == "stat"
        {
            lexical = prev_of(&lexical);
        }
        let lb = lexical.borrow();
        let ty = lb.ty;
        let closing = first_char.is_some_and(|c| ty.encode_utf16().eq(std::iter::once(c)));
        let unit = self.indent_unit;
        Some(if ty == "vardef" {
            let cont = state.last_type == Some("operator") || state.last_type == Some(",");
            lb.indented.plus(if cont {
                lb.info.as_ref().expect("TypeError: info").len() as f64 + 1.0
            } else {
                0.0
            })
        } else if ty == "form" && first_char == Some(b'{' as u16) {
            return lb.indented.value();
        } else if ty == "form" {
            lb.indented.plus(unit)
        } else if ty == "stat" {
            // isContinuedStatement
            let cont = state.last_type == Some("operator")
                || state.last_type == Some(",")
                || first_char.is_some_and(is_operator_char)
                || first_char == Some(b',' as u16)
                || first_char == Some(b'.' as u16);
            lb.indented.plus(if cont {
                self.statement_indent.unwrap_or(unit)
            } else {
                0.0
            })
        } else if lb.info.as_deref().is_some_and(|i| eq(i, "switch"))
            && !closing
            && self.double_indent_switch
        {
            lb.indented.plus(if starts_case_default(text_after) {
                unit
            } else {
                2.0 * unit
            })
        } else if lb.align == Some(true) {
            lb.column + if closing { 0.0 } else { 1.0 }
        } else {
            lb.indented.plus(if closing { 0.0 } else { unit })
        })
    }

    /// `electricInput: /^\s*(?:case .*?:|default:|\{|\})$/`
    fn electric_input(&self, text: &[u16]) -> Option<bool> {
        let s = &text[spaces(text, 0)..];
        let r = eq(s, "default:")
            || eq(s, "{")
            || eq(s, "}")
            || (s.len() >= 6
                && eq(&s[..5], "case ")
                && s[s.len() - 1] == b':' as u16
                && s[5..s.len() - 1]
                    .iter()
                    .all(|&c| cm_core::text::is_js_dot(c)));
        Some(r)
    }

    fn block_comment_start(&self) -> Option<&str> {
        (!self.json_mode).then_some("/*")
    }
    fn block_comment_end(&self) -> Option<&str> {
        (!self.json_mode).then_some("*/")
    }
    fn line_comment(&self) -> Option<&str> {
        (!self.json_mode).then_some("//")
    }
    fn helper_type(&self) -> Option<&str> {
        Some(if self.json_mode { "json" } else { "javascript" })
    }
    fn prop(&self, name: &str) -> Option<ConfigValue> {
        match name {
            "blockCommentContinue" => Some(if self.json_mode {
                ConfigValue::Null
            } else {
                ConfigValue::Str(" * ".into())
            }),
            "fold" => Some(ConfigValue::Str("brace".into())),
            "closeBrackets" => Some(ConfigValue::Str("()[]{}''\"\"``".into())),
            "jsonldMode" => Some(ConfigValue::Bool(self.jsonld_mode)),
            "jsonMode" => Some(ConfigValue::Bool(self.json_mode)),
            _ => None,
        }
    }
}

/// `CodeMirror.defineMode("javascript", ...)` and the file's `defineMIME` calls.
pub fn register(r: &mut Registry) {
    r.define_mode(
        "javascript",
        Rc::new(|_, config, spec| Rc::new(JsMode::new(config, spec)) as Rc<dyn Mode>),
    );
    // `CodeMirror.registerHelper("wordChars", "javascript", /[\w$]/)`
    r.helpers.register(
        "wordChars",
        "javascript",
        Rc::new(crate::RegExp::new(r"[\w$]", "").expect("wordChars regexp")),
    );
    for mime in [
        "text/javascript",
        "text/ecmascript",
        "application/javascript",
        "application/x-javascript",
        "application/ecmascript",
    ] {
        r.define_mime(mime, MimeTarget::Name("javascript".into()));
    }
    let js =
        |k: &str| MimeTarget::Spec(ModeSpec::named("javascript").with(k, ConfigValue::Bool(true)));
    r.define_mime("application/json", js("json"));
    r.define_mime("application/x-json", js("json"));
    r.define_mime("application/manifest+json", js("json"));
    r.define_mime("application/ld+json", js("jsonld"));
    r.define_mime("text/typescript", js("typescript"));
    r.define_mime("application/typescript", js("typescript"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use cm_core::text::to_u16;

    fn len(f: fn(&[u16]) -> Option<RegexMatch>, s: &str) -> Option<usize> {
        f(&to_u16(s)).map(|m| m.len)
    }

    #[test]
    fn regexes() {
        assert_eq!(len(re_regexp_flags, "gi;"), Some(2));
        assert_eq!(len(re_regexp_flags, "gg"), None);
        assert_eq!(len(re_regexp_flags, "gix"), None);
        assert_eq!(len(re_regexp_flags, "gig"), None);
        assert_eq!(len(re_decimal_rest, "12.5e+3x"), Some(7));
        assert_eq!(len(re_decimal_rest, "1e"), Some(1));
        assert_eq!(len(re_decimal_rest, "1n"), Some(2));
        assert_eq!(len(re_radix, "x1Fn"), Some(4));
        assert_eq!(len(re_async_lookahead, " /* a */ (x"), Some(10));
        assert_eq!(len(re_async_lookahead, " /* a  (x"), None);
        assert_eq!(len(re_ts_call_type_args, "a<b>>(x"), Some(6));
        assert_eq!(len(re_jsonld_keyword, "@id\""), Some(4));
        assert_eq!(re_ts_arrow_return(&to_u16("(a): Foo<x> ")), Some(3));
        assert_eq!(len(re_type_predicate, " x is y"), Some(5));
    }
}
