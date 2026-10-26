//! Port of `addon/mode/simple.js` (CodeMirror 5.65.21): `CodeMirror.defineSimpleMode`
//! and `CodeMirror.simpleMode`.
//!
//! A simple mode is a table of states, each a list of rules `{regex, token, next,
//! push, pop, sol, mode, indent, dedent, dedentIfLineStart}`, plus a `meta` object
//! whose properties are copied onto the mode. The first rule whose regexp matches
//! at the stream position wins.
//!
//! Rust shapes of the JS values: a rule is [`Rule`]; `regex` is [`RegexSpec`] (a
//! string or a [`RegExp`]); `token` is [`TokenSpec`]; `mode` is [`LocalModeSpec`].
//! `meta` is a map of [`ConfigValue`]s: `lineComment`, `blockCommentStart`,
//! `blockCommentEnd`, `electricChars` (strings) also answer the typed accessors,
//! `electricInput` is `ConfigValue::Obj(Rc<RegExp>)`, `dontIndentStates` is
//! `ConfigValue::Obj(Rc<Vec<String>>)`.
//!
//! JS behavior kept on purpose:
//! - every rule regexp is `^(?:source)` with only the `i` and `u` flags of a given
//!   [`RegExp`]; a falsy regex (`None`, `""`) is the unanchored `/(?:)/`, which
//!   matches the empty string;
//! - string tokens have every `.` replaced by a space; array tokens keep `null`
//!   and `""` entries as they are; a function token's result is used raw;
//! - with two or more capture groups and a non-string token, groups 2.. become
//!   pending tokens (empty groups skipped) and the stream backs up to the end of
//!   group 1; the first token returned is `token[0]` (for a function returning a
//!   string, its first character);
//! - `indent` ignores the `next`/`push` of a dedent rule (upstream reads
//!   `rule.next`, which is always undefined), so it never switches rule lists;
//! - a `meta` object makes `"meta"` a valid (empty) state name, since the JS loop
//!   compares the state name to the meta object;
//! - `pending` is copied by reference in JS; here it is copied by value. Pending
//!   tokens never outlive the line that made them, so no copy can observe the
//!   difference.
//!
//! Differences: rule regexps (and local-mode `end` patterns) are compiled when the
//! mode is defined, so a bad pattern or an undefined state is an error from
//! [`define_simple_mode`] / [`simple_mode`] instead of an exception from `getMode`
//! or `enterLocalMode`. A function token used as a local mode's `endToken`, or a
//! function token whose result is null in the capture-group branch (a `TypeError`
//! in JS), gives a null style. `cmp` on persistent local-mode specs is
//! [`Spec`]'s `PartialEq` (object-valued options compare by identity).

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use cm_core::util::string_stream::StringStream;

use crate::{
    ConfigValue, JsArg, Mode, ModeFactory, ModeOptions, RegExp, RegExpError, Registry, Spec, State,
    copy_state, start_state,
};

/// A rule's `regex` (or a local mode's `end`).
#[derive(Clone, Debug, Default)]
pub enum RegexSpec {
    /// undefined / null / false.
    #[default]
    None,
    /// A string: its text is the regexp source.
    Str(String),
    Re(Rc<RegExp>),
}

impl RegexSpec {
    pub fn re(source: &str, flags: &str) -> Result<RegexSpec, RegExpError> {
        Ok(RegexSpec::Re(Rc::new(RegExp::new(source, flags)?)))
    }
}

/// What a function token returns.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenValue {
    Null,
    Str(String),
    List(Vec<Option<String>>),
}

/// The matched text of each group: `matches[i]` (`None` = undefined).
pub type Matches = [Option<Vec<u16>>];

/// A rule's `token`.
#[derive(Clone, Default)]
pub enum TokenSpec {
    #[default]
    Null,
    Str(String),
    /// An array of styles, one per capture group.
    List(Vec<Option<String>>),
    /// `token(matches)`.
    Fn(Rc<dyn Fn(&Matches) -> TokenValue>),
}

impl fmt::Debug for TokenSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenSpec::Null => write!(f, "Null"),
            TokenSpec::Str(s) => write!(f, "Str({s:?})"),
            TokenSpec::List(l) => write!(f, "List({l:?})"),
            TokenSpec::Fn(_) => write!(f, "Fn(..)"),
        }
    }
}

/// A rule's `mode`: `{spec, mode, end, persistent, forceEnd}`.
#[derive(Clone, Default)]
pub struct LocalModeSpec {
    /// `spec`: passed to `getMode` when `mode` is not given.
    pub spec: Option<Spec>,
    /// `mode`: a mode object to use as is.
    pub mode: Option<Rc<dyn Mode>>,
    pub end: RegexSpec,
    pub persistent: bool,
    /// `forceEnd`; only `Some(false)` turns the end scan off.
    pub force_end: Option<bool>,
}

/// One rule.
#[derive(Clone, Default)]
pub struct Rule {
    pub regex: RegexSpec,
    pub token: TokenSpec,
    pub sol: bool,
    pub next: Option<String>,
    pub push: Option<String>,
    pub pop: bool,
    pub mode: Option<LocalModeSpec>,
    pub indent: bool,
    pub dedent: bool,
    /// `dedentIfLineStart`; only `Some(false)` disables it.
    pub dedent_if_line_start: Option<bool>,
}

impl Rule {
    /// `{regex, token}`.
    pub fn new(regex: RegexSpec, token: TokenSpec) -> Rule {
        Rule {
            regex,
            token,
            ..Default::default()
        }
    }
}

/// The `states` argument: rule lists by state name, plus `meta`.
#[derive(Clone, Default)]
pub struct SimpleStates {
    pub states: Vec<(String, Vec<Rule>)>,
    /// `states.meta`; `None` = absent.
    pub meta: Option<HashMap<String, ConfigValue>>,
}

/// `new Error("Undefined state " + name + " in simple mode")`, or a bad regexp.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimpleModeError {
    UndefinedState(String),
    RegExp(RegExpError),
}

impl fmt::Display for SimpleModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimpleModeError::UndefinedState(s) => write!(f, "Undefined state {s} in simple mode"),
            SimpleModeError::RegExp(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for SimpleModeError {}

impl From<RegExpError> for SimpleModeError {
    fn from(e: RegExpError) -> Self {
        SimpleModeError::RegExp(e)
    }
}

// --------------------------------------------------------------------------------
// Compiled tables

/// `asToken(val)`.
#[derive(Clone)]
enum Token {
    Null,
    Str(String),
    List(Vec<Option<String>>),
    Fn(Rc<dyn Fn(&Matches) -> TokenValue>),
}

#[derive(Clone)]
struct CompiledLocal {
    spec: LocalModeSpec,
    end: Option<Rc<RegExp>>,
    end_scan: Option<Rc<RegExp>>,
}

struct CompiledRule {
    regex: Rc<RegExp>,
    token: Token,
    data: Rule,
    local: Option<CompiledLocal>,
}

struct Compiled {
    states: HashMap<String, Vec<CompiledRule>>,
    meta: HashMap<String, ConfigValue>,
    has_indentation: bool,
}

/// `toRegex(val, caret)`.
fn to_regex(val: &RegexSpec, caret: bool) -> Result<Rc<RegExp>, RegExpError> {
    let (source, flags): (Vec<u16>, String) = match val {
        RegexSpec::None => return Ok(Rc::new(RegExp::new("(?:)", "")?)),
        RegexSpec::Str(s) if s.is_empty() => return Ok(Rc::new(RegExp::new("(?:)", "")?)),
        RegexSpec::Str(s) => (s.encode_utf16().collect(), String::new()),
        RegexSpec::Re(r) => {
            let mut flags = String::new();
            if r.ignore_case() {
                flags.push('i');
            }
            if r.unicode() {
                flags.push('u');
            }
            (r.source().to_vec(), flags)
        }
    };
    let mut src: Vec<u16> = if caret { "^(?:" } else { "(?:" }.encode_utf16().collect();
    src.extend_from_slice(&source);
    src.push(')' as u16);
    Ok(Rc::new(RegExp::from_utf16(&src, &flags)?))
}

fn dots_to_spaces(s: &str) -> String {
    s.replace('.', " ")
}

/// `asToken(val)`.
fn as_token(val: &TokenSpec) -> Token {
    match val {
        TokenSpec::Null => Token::Null,
        TokenSpec::Str(s) if s.is_empty() => Token::Null,
        TokenSpec::Str(s) => Token::Str(dots_to_spaces(s)),
        TokenSpec::List(l) => Token::List(
            l.iter()
                .map(|t| match t {
                    Some(s) if !s.is_empty() => Some(dots_to_spaces(s)),
                    other => other.clone(),
                })
                .collect(),
        ),
        TokenSpec::Fn(f) => Token::Fn(f.clone()),
    }
}

fn compile(states: &SimpleStates) -> Result<Compiled, SimpleModeError> {
    let defined = |name: &str| {
        states.states.iter().any(|(n, _)| n == name) || (name == "meta" && states.meta.is_some())
    };
    let ensure = |name: &str| {
        if defined(name) {
            Ok(())
        } else {
            Err(SimpleModeError::UndefinedState(name.to_string()))
        }
    };
    ensure("start")?;
    let mut out = HashMap::new();
    let mut has_indentation = false;
    if states.meta.is_some() {
        out.insert("meta".to_string(), Vec::new());
    }
    for (name, rules) in &states.states {
        let mut list = Vec::new();
        for data in rules {
            if let Some(target) = data.next.as_ref().or(data.push.as_ref()) {
                ensure(target)?;
            }
            let local = match &data.mode {
                Some(spec) => {
                    let has_end = !matches!(&spec.end, RegexSpec::None)
                        && !matches!(&spec.end, RegexSpec::Str(s) if s.is_empty());
                    Some(CompiledLocal {
                        spec: spec.clone(),
                        end: if has_end {
                            Some(to_regex(&spec.end, true)?)
                        } else {
                            None
                        },
                        end_scan: if has_end && spec.force_end != Some(false) {
                            Some(to_regex(&spec.end, false)?)
                        } else {
                            None
                        },
                    })
                }
                None => None,
            };
            list.push(CompiledRule {
                regex: to_regex(&data.regex, true)?,
                token: as_token(&data.token),
                data: data.clone(),
                local,
            });
            if data.indent || data.dedent {
                has_indentation = true;
            }
        }
        out.insert(name.clone(), list);
    }
    Ok(Compiled {
        states: out,
        meta: states.meta.clone().unwrap_or_default(),
        has_indentation,
    })
}

// --------------------------------------------------------------------------------
// State

/// `state.local`: the active local mode (shared, never changed, as in JS).
struct ActiveLocal {
    mode: Rc<dyn Mode>,
    end: Option<Rc<RegExp>>,
    end_scan: Option<Rc<RegExp>>,
    end_token: Option<String>,
}

/// One `persistentStates` entry.
struct Persistent {
    mode: Rc<dyn Mode>,
    spec: Option<Spec>,
    state: Rc<RefCell<State>>,
    next: Option<Rc<Persistent>>,
}

/// The simple mode's state. `local_state` is shared with a persistent entry, as the
/// JS object is.
#[derive(Clone)]
pub struct SimpleState {
    /// `state.state`: the current state name.
    pub state: String,
    pub pending: Option<Vec<(usize, Option<String>)>>,
    local: Option<Rc<ActiveLocal>>,
    local_state: Option<Rc<RefCell<State>>>,
    /// `state.indent`: `None` when no rule indents.
    pub indent: Option<Vec<f64>>,
    /// `state.stack` (`push` / `pop`).
    pub stack: Option<Vec<String>>,
    persistent_states: Option<Rc<Persistent>>,
}

impl fmt::Debug for SimpleState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SimpleState")
            .field("state", &self.state)
            .field("pending", &self.pending)
            .field(
                "local",
                &self.local.as_ref().map(|l| l.mode.name().to_string()),
            )
            .field("local_state", &self.local_state)
            .field("indent", &self.indent)
            .field("stack", &self.stack)
            .finish_non_exhaustive()
    }
}

impl SimpleState {
    /// The active local mode's name, if any.
    pub fn local_mode_name(&self) -> Option<&str> {
        self.local.as_ref().map(|l| l.mode.name())
    }
    /// The number of `persistentStates` entries.
    pub fn persistent_count(&self) -> usize {
        let mut n = 0;
        let mut p = self.persistent_states.as_ref();
        while let Some(e) = p {
            n += 1;
            p = e.next.as_ref();
        }
        n
    }
}

// --------------------------------------------------------------------------------
// Mode

/// `CodeMirror.simpleMode(config, states)`.
pub struct SimpleMode {
    compiled: Rc<Compiled>,
    registry: Registry,
    config: ModeOptions,
}

/// `CodeMirror.simpleMode(config, states)`. The registry is used for local modes
/// given by `spec` (`CodeMirror.getMode(config, spec.spec)`).
pub fn simple_mode(
    registry: &Registry,
    config: &ModeOptions,
    states: &SimpleStates,
) -> Result<SimpleMode, SimpleModeError> {
    Ok(SimpleMode {
        compiled: Rc::new(compile(states)?),
        registry: registry.clone(),
        config: config.clone(),
    })
}

/// `CodeMirror.defineSimpleMode(name, states)`. The tables are checked and compiled
/// here, once; each `getMode` shares them.
pub fn define_simple_mode(
    registry: &mut Registry,
    name: &str,
    states: SimpleStates,
) -> Result<(), SimpleModeError> {
    let compiled = Rc::new(compile(&states)?);
    let factory: ModeFactory = Rc::new(move |registry, config, _spec| {
        Rc::new(SimpleMode {
            compiled: compiled.clone(),
            registry: registry.clone(),
            config: config.clone(),
        }) as Rc<dyn Mode>
    });
    registry.define_mode(name, factory);
    Ok(())
}

impl SimpleMode {
    fn meta_str(&self, name: &str) -> Option<&str> {
        self.compiled.meta.get(name).and_then(ConfigValue::as_str)
    }

    /// `enterLocalMode(config, state, spec, token)`.
    fn enter_local_mode(&self, state: &mut SimpleState, local: &CompiledLocal, token: &Token) {
        let spec = &local.spec;
        let mut pers: Option<Rc<Persistent>> = None;
        if spec.persistent {
            let mut p = state.persistent_states.clone();
            while let Some(e) = p {
                if pers.is_some() {
                    break;
                }
                let same = match &spec.spec {
                    Some(s) => e.spec.as_ref() == Some(s),
                    None => spec.mode.as_ref().is_some_and(|m| Rc::ptr_eq(m, &e.mode)),
                };
                if same {
                    pers = Some(e.clone());
                }
                p = e.next.clone();
            }
        }
        let mode = match (&pers, &spec.mode) {
            (Some(p), _) => p.mode.clone(),
            (None, Some(m)) => m.clone(),
            (None, None) => self
                .registry
                .get_mode(&self.config, spec.spec.as_ref().unwrap_or(&Spec::Null)),
        };
        let l_state = match &pers {
            Some(p) => p.state.clone(),
            None => Rc::new(RefCell::new(start_state(
                &*mode,
                JsArg::Undefined,
                JsArg::Undefined,
            ))),
        };
        if spec.persistent && pers.is_none() {
            state.persistent_states = Some(Rc::new(Persistent {
                mode: mode.clone(),
                spec: spec.spec.clone(),
                state: l_state.clone(),
                next: state.persistent_states.take(),
            }));
        }
        state.local_state = Some(l_state);
        state.local = Some(Rc::new(ActiveLocal {
            mode,
            end: local.end.clone(),
            end_scan: local.end_scan.clone(),
            end_token: match token {
                Token::List(l) => l.last().cloned().flatten(),
                Token::Str(s) => Some(s.clone()),
                Token::Null | Token::Fn(_) => None,
            },
        }));
    }
}

/// `token[0]` of a token value.
fn first_of(v: &TokenValue) -> Option<String> {
    match v {
        TokenValue::Null => None,
        TokenValue::Str(s) => {
            let u: Vec<u16> = s.encode_utf16().take(1).collect();
            (!u.is_empty()).then(|| String::from_utf16_lossy(&u))
        }
        TokenValue::List(l) => l.first().cloned().flatten(),
    }
}

impl Mode for SimpleMode {
    fn name(&self) -> &str {
        ""
    }
    fn start_state(&self, _a1: JsArg, _a2: JsArg) -> Option<State> {
        Some(State::new(SimpleState {
            state: "start".to_string(),
            pending: None,
            local: None,
            local_state: None,
            indent: self.compiled.has_indentation.then(Vec::new),
            stack: None,
            persistent_states: None,
        }))
    }
    fn copy_state(&self, state: &State) -> Option<State> {
        let st = state.get::<SimpleState>();
        let local_state = st.local_state.as_ref().map(|ls| {
            let mode = &st
                .local
                .as_ref()
                .expect("a local state has a local mode")
                .mode;
            Rc::new(RefCell::new(copy_state(&**mode, &ls.borrow())))
        });
        // The JS loop prepends, so the copied list is in reverse order.
        let mut persistent_states: Option<Rc<Persistent>> = None;
        let mut p = st.persistent_states.as_ref();
        while let Some(e) = p {
            let shared = match (&st.local_state, &local_state) {
                (Some(old), Some(new)) if Rc::ptr_eq(&e.state, old) => Some(new.clone()),
                _ => None,
            };
            persistent_states = Some(Rc::new(Persistent {
                mode: e.mode.clone(),
                spec: e.spec.clone(),
                state: shared.unwrap_or_else(|| {
                    Rc::new(RefCell::new(copy_state(&*e.mode, &e.state.borrow())))
                }),
                next: persistent_states.take(),
            }));
            p = e.next.as_ref();
        }
        Some(State::new(SimpleState {
            state: st.state.clone(),
            pending: st.pending.clone(),
            local: st.local.clone(),
            local_state,
            indent: st.indent.clone(),
            stack: st.stack.clone(),
            persistent_states,
        }))
    }
    fn token(&self, stream: &mut StringStream<'_>, state: &mut State) -> Option<String> {
        let st = state.get_mut::<SimpleState>();
        if let Some(pending) = &mut st.pending {
            let (len, token) = pending.remove(0);
            if pending.is_empty() {
                st.pending = None;
            }
            stream.pos += len;
            return token;
        }

        if let Some(local) = st.local.clone() {
            if let Some(end) = &local.end
                && stream.match_regexp(end, true).is_some()
            {
                st.local = None;
                st.local_state = None;
                return local.end_token.clone();
            }
            let ls = st.local_state.clone().expect("a local mode has a state");
            let tok = local.mode.token(stream, &mut ls.borrow_mut());
            if let Some(scan) = &local.end_scan
                && let Some(m) = scan.exec(stream.current())
            {
                stream.pos = stream.start + m.index();
            }
            return tok;
        }

        let cur_state = &self.compiled.states[&st.state];
        for rule in cur_state {
            if rule.data.sol && !stream.sol() {
                continue;
            }
            let rest = &stream.string[stream.pos.min(stream.string.len())..];
            let Some(m) = stream.match_regexp(&rule.regex, true) else {
                continue;
            };
            let matches: Vec<Option<Vec<u16>>> = (0..m.count())
                .map(|i| m.text(rest, i).map(<[u16]>::to_vec))
                .collect();
            if let Some(next) = &rule.data.next {
                st.state = next.clone();
            } else if let Some(push) = &rule.data.push {
                st.stack
                    .get_or_insert_with(Vec::new)
                    .push(std::mem::replace(&mut st.state, push.clone()));
            } else if rule.data.pop
                && let Some(prev) = st.stack.as_mut().and_then(Vec::pop)
            {
                st.state = prev;
            }

            if let Some(local) = &rule.local {
                self.enter_local_mode(st, local, &rule.token);
            }
            if rule.data.indent
                && let Some(ind) = &mut st.indent
            {
                ind.push((stream.indentation() + self.config.indent_unit) as f64);
            }
            if rule.data.dedent
                && let Some(ind) = &mut st.indent
            {
                ind.pop();
            }
            let token = match &rule.token {
                Token::Null => TokenValue::Null,
                Token::Str(s) => TokenValue::Str(s.clone()),
                Token::List(l) => TokenValue::List(l.clone()),
                Token::Fn(f) => f(&matches),
            };
            let token_is_non_string = !matches!(rule.token, Token::Null | Token::Str(_));
            if matches.len() > 2 && token_is_non_string {
                for (j, text) in matches.iter().enumerate().skip(2) {
                    if let Some(text) = text.as_ref().filter(|t| !t.is_empty()) {
                        let style = match &rule.token {
                            Token::List(l) => l.get(j - 1).cloned().flatten(),
                            _ => None,
                        };
                        st.pending
                            .get_or_insert_with(Vec::new)
                            .push((text.len(), style));
                    }
                }
                let g1 = matches[1].as_ref().map_or(0, Vec::len);
                stream.back_up(m.len() - g1);
                return first_of(&token);
            }
            return match token {
                TokenValue::List(l) => l.first().cloned().flatten(),
                TokenValue::Str(s) => Some(s),
                TokenValue::Null => None,
            };
        }
        stream.next();
        None
    }
    fn has_inner_mode(&self) -> bool {
        true
    }
    fn inner_mode(&self, state: &State) -> Option<(Rc<dyn Mode>, State)> {
        let st = state.get::<SimpleState>();
        let local = st.local.as_ref()?;
        Some((
            local.mode.clone(),
            st.local_state.as_ref().unwrap().borrow().clone(),
        ))
    }
    fn inner_mode_mut(
        &self,
        state: &mut State,
        f: &mut dyn FnMut(&Rc<dyn Mode>, &mut State),
    ) -> bool {
        let st = state.get_mut::<SimpleState>();
        let Some(local) = &st.local else {
            return false;
        };
        f(
            &local.mode,
            &mut st.local_state.as_ref().unwrap().borrow_mut(),
        );
        true
    }
    fn has_indent(&self) -> bool {
        true
    }
    /// `indentFunction(states, meta)`.
    fn indent(&self, state: &State, text_after: &[u16], line: &[u16]) -> Option<f64> {
        let st = state.get::<SimpleState>();
        if let Some(local) = &st.local
            && local.mode.has_indent()
        {
            return local
                .mode
                .indent(&st.local_state.as_ref().unwrap().borrow(), text_after, line);
        }
        let dont_indent = self
            .compiled
            .meta
            .get("dontIndentStates")
            .and_then(|v| v.as_obj::<Vec<String>>())
            .is_some_and(|l| l.contains(&st.state));
        let indent = st.indent.as_ref()?;
        if st.local.is_some() || dont_indent {
            return None;
        }
        let mut pos = indent.len() as i64 - 1;
        let rules = &self.compiled.states[&st.state];
        let mut text_after = text_after;
        'scan: loop {
            for rule in rules {
                if rule.data.dedent
                    && rule.data.dedent_if_line_start != Some(false)
                    && let Some(m) = rule.regex.exec(text_after)
                    && !m.is_empty()
                {
                    pos -= 1;
                    text_after = &text_after[m.len().min(text_after.len())..];
                    continue 'scan;
                }
            }
            break;
        }
        Some(if pos < 0 { 0.0 } else { indent[pos as usize] })
    }
    fn electric_input(&self, text: &[u16]) -> Option<bool> {
        let re = self
            .compiled
            .meta
            .get("electricInput")?
            .as_obj::<RegExp>()?;
        Some(re.test(text))
    }
    fn prop(&self, name: &str) -> Option<ConfigValue> {
        self.compiled.meta.get(name).cloned()
    }
    fn electric_chars(&self) -> Option<&str> {
        self.meta_str("electricChars")
    }
    fn line_comment(&self) -> Option<&str> {
        self.meta_str("lineComment")
    }
    fn block_comment_start(&self) -> Option<&str> {
        self.meta_str("blockCommentStart")
    }
    fn block_comment_end(&self) -> Option<&str> {
        self.meta_str("blockCommentEnd")
    }
}
