//! Port of `addon/mode/multiplex.js` (CodeMirror 5.65.21):
//! `CodeMirror.multiplexingMode(outer, ...others)`.
//!
//! An outer mode with inner modes between `open` and `close` delimiters. While no
//! inner mode is active, the outer mode only sees the line up to the next opening
//! delimiter (the stream's string is cut there and restored after the token); an
//! active inner mode likewise sees the line only up to its closing delimiter.
//!
//! Delimiters are strings or regexps ([`Delimiter`]). A regexp is run with `exec`
//! on `string.slice(from)`, as the JS `indexOf` helper does. A `g` regexp's
//! `lastIndex` is not carried between calls (no upstream mode uses one).

use std::rc::Rc;

use cm_core::text::index_of;
use cm_core::util::string_stream::StringStream;

use crate::{JsArg, Mode, RegExp, State, copy_state, start_state};

/// An `open` / `close` value.
#[derive(Clone, Debug)]
pub enum Delimiter {
    Str(Vec<u16>),
    Re(Rc<RegExp>),
}

impl Delimiter {
    pub fn str(s: &str) -> Delimiter {
        Delimiter::Str(s.encode_utf16().collect())
    }
    pub fn re(r: RegExp) -> Delimiter {
        Delimiter::Re(Rc::new(r))
    }
    /// JS truthiness: an empty string is false.
    fn truthy(&self) -> bool {
        !matches!(self, Delimiter::Str(s) if s.is_empty())
    }
    fn is_newline(&self) -> bool {
        matches!(self, Delimiter::Str(s) if s.as_slice() == [0x0a])
    }
}

/// One of `others`: `{open, close, mode, delimStyle, innerStyle, parseDelimiters}`.
#[derive(Clone)]
pub struct InnerMode {
    pub open: Delimiter,
    /// `None` = undefined: the inner mode ends at the next line start.
    pub close: Option<Delimiter>,
    pub mode: Rc<dyn Mode>,
    pub delim_style: Option<String>,
    pub inner_style: Option<String>,
    pub parse_delimiters: bool,
}

impl InnerMode {
    pub fn new(open: Delimiter, close: Option<Delimiter>, mode: Rc<dyn Mode>) -> InnerMode {
        InnerMode {
            open,
            close,
            mode,
            delim_style: None,
            inner_style: None,
            parse_delimiters: false,
        }
    }
    fn close_truthy(&self) -> Option<&Delimiter> {
        self.close.as_ref().filter(|c| c.truthy())
    }
}

/// The multiplexing mode.
pub struct MultiplexMode {
    outer: Rc<dyn Mode>,
    others: Vec<InnerMode>,
}

/// The multiplexing mode's state. `inner_active` indexes `others`.
#[derive(Clone, Debug)]
pub struct MultiplexState {
    pub outer: State,
    pub inner_active: Option<usize>,
    pub inner: Option<State>,
    pub starting_inner: bool,
}

/// `CodeMirror.multiplexingMode(outer, ...others)`.
pub fn multiplexing_mode(outer: Rc<dyn Mode>, others: Vec<InnerMode>) -> Rc<dyn Mode> {
    Rc::new(MultiplexMode { outer, others })
}

/// The helper `indexOf(string, pattern, from, returnEnd)`: -1 for no match.
fn index_of_delim(string: &[u16], pattern: &Delimiter, from: usize, return_end: bool) -> i64 {
    match pattern {
        Delimiter::Str(p) => match index_of(string, p, from) {
            Some(f) if return_end => (f + p.len()) as i64,
            Some(f) => f as i64,
            None => -1,
        },
        Delimiter::Re(re) => {
            let from = from.min(string.len());
            match re.exec(&string[from..]) {
                Some(m) => (m.index() + from + if return_end { m.len() } else { 0 }) as i64,
                None => -1,
            }
        }
    }
}

/// `stream.match(pattern)` (consuming).
fn match_delim(stream: &mut StringStream<'_>, pattern: &Delimiter) {
    match pattern {
        Delimiter::Str(p) => {
            stream.match_str(&String::from_utf16_lossy(p), true, false);
        }
        Delimiter::Re(re) => {
            stream.match_regexp(re, true);
        }
    }
}

/// `style && (style + " " + style + suffix)`.
fn delim_token(style: &Option<String>, suffix: &str) -> Option<String> {
    match style {
        Some(s) if !s.is_empty() => Some(format!("{s} {s}{suffix}")),
        other => other.clone(),
    }
}

impl Mode for MultiplexMode {
    fn name(&self) -> &str {
        ""
    }
    fn start_state(&self, _a1: JsArg, _a2: JsArg) -> Option<State> {
        Some(State::new(MultiplexState {
            outer: start_state(&*self.outer, JsArg::Undefined, JsArg::Undefined),
            inner_active: None,
            inner: None,
            starting_inner: false,
        }))
    }
    fn copy_state(&self, state: &State) -> Option<State> {
        let s = state.get::<MultiplexState>();
        Some(State::new(MultiplexState {
            outer: copy_state(&*self.outer, &s.outer),
            inner_active: s.inner_active,
            inner: s.inner_active.and_then(|i| {
                s.inner
                    .as_ref()
                    .map(|inner| copy_state(&*self.others[i].mode, inner))
            }),
            starting_inner: s.starting_inner,
        }))
    }
    fn token(&self, stream: &mut StringStream<'_>, state: &mut State) -> Option<String> {
        let s = state.get_mut::<MultiplexState>();
        let old_content = stream.string;
        let Some(cur) = s.inner_active else {
            let mut cut_off: Option<usize> = None;
            for (i, other) in self.others.iter().enumerate() {
                let found = index_of_delim(old_content, &other.open, stream.pos, false);
                if found == stream.pos as i64 {
                    if !other.parse_delimiters {
                        match_delim(stream, &other.open);
                    }
                    s.starting_inner = other.parse_delimiters;
                    s.inner_active = Some(i);

                    // Get the outer indent, making sure to handle CodeMirror.Pass
                    let mut outer_indent = 0.0;
                    if self.outer.has_indent()
                        && let Some(x) = self.outer.indent(&s.outer, &[], &[])
                    {
                        outer_indent = x;
                    }

                    s.inner = Some(start_state(
                        &*other.mode,
                        JsArg::Num(outer_indent),
                        JsArg::Undefined,
                    ));
                    return delim_token(&other.delim_style, "-open");
                } else if found != -1 && cut_off.is_none_or(|c| (found as usize) < c) {
                    cut_off = Some(found as usize);
                }
            }
            if let Some(c) = cut_off {
                stream.string = &old_content[..c];
            }
            let outer_token = self.outer.token(stream, &mut s.outer);
            if cut_off.is_some() {
                stream.string = old_content;
            }
            return outer_token;
        };
        let cur_inner = &self.others[cur];
        if cur_inner.close_truthy().is_none() && stream.sol() {
            s.inner_active = None;
            s.inner = None;
            return self.token(stream, state);
        }
        let found = match cur_inner.close_truthy() {
            Some(close) if !s.starting_inner => {
                index_of_delim(old_content, close, stream.pos, cur_inner.parse_delimiters)
            }
            _ => -1,
        };
        if found == stream.pos as i64 && !cur_inner.parse_delimiters {
            match_delim(stream, cur_inner.close.as_ref().unwrap());
            s.inner_active = None;
            s.inner = None;
            return delim_token(&cur_inner.delim_style, "-close");
        }
        if found > -1 {
            stream.string = &old_content[..found as usize];
        }
        let inner_state = s.inner.as_mut().expect("an active inner mode has a state");
        let mut inner_token = cur_inner.mode.token(stream, inner_state);
        if found > -1 {
            stream.string = old_content;
        } else if stream.pos > stream.start {
            s.starting_inner = false;
        }

        if found == stream.pos as i64 && cur_inner.parse_delimiters {
            s.inner_active = None;
            s.inner = None;
        }

        if let Some(style) = cur_inner.inner_style.as_ref().filter(|x| !x.is_empty()) {
            inner_token = match inner_token {
                Some(t) if !t.is_empty() => Some(format!("{t} {style}")),
                _ => Some(style.clone()),
            };
        }

        inner_token
    }
    fn has_indent(&self) -> bool {
        true
    }
    fn indent(&self, state: &State, text_after: &[u16], line: &[u16]) -> Option<f64> {
        let s = state.get::<MultiplexState>();
        let (mode, st) = match s.inner_active {
            Some(i) => (&self.others[i].mode, s.inner.as_ref().unwrap()),
            None => (&self.outer, &s.outer),
        };
        if !mode.has_indent() {
            return None;
        }
        mode.indent(st, text_after, line)
    }
    fn has_blank_line(&self) -> bool {
        true
    }
    fn blank_line(&self, state: &mut State) -> Option<String> {
        let s = state.get_mut::<MultiplexState>();
        let mode = match s.inner_active {
            Some(i) => self.others[i].mode.clone(),
            None => self.outer.clone(),
        };
        if mode.has_blank_line() {
            match s.inner_active {
                Some(_) => mode.blank_line(s.inner.as_mut().unwrap()),
                None => mode.blank_line(&mut s.outer),
            };
        }
        match s.inner_active {
            None => {
                for (i, other) in self.others.iter().enumerate() {
                    if other.open.is_newline() {
                        let a1 = if mode.has_indent() {
                            JsArg::from(mode.indent(&s.outer, &[], &[]))
                        } else {
                            JsArg::Num(0.0)
                        };
                        s.inner_active = Some(i);
                        s.inner = Some(start_state(&*other.mode, a1, JsArg::Undefined));
                    }
                }
            }
            Some(i) => {
                if self.others[i]
                    .close
                    .as_ref()
                    .is_some_and(Delimiter::is_newline)
                {
                    s.inner_active = None;
                    s.inner = None;
                }
            }
        }
        None
    }
    fn electric_chars(&self) -> Option<&str> {
        self.outer.electric_chars()
    }
    fn has_inner_mode(&self) -> bool {
        true
    }
    fn inner_mode(&self, state: &State) -> Option<(Rc<dyn Mode>, State)> {
        let s = state.get::<MultiplexState>();
        Some(match (&s.inner, s.inner_active) {
            (Some(inner), Some(i)) => (self.others[i].mode.clone(), inner.clone()),
            _ => (self.outer.clone(), s.outer.clone()),
        })
    }
    fn inner_mode_mut(
        &self,
        state: &mut State,
        f: &mut dyn FnMut(&Rc<dyn Mode>, &mut State),
    ) -> bool {
        let s = state.get_mut::<MultiplexState>();
        match (&mut s.inner, s.inner_active) {
            (Some(inner), Some(i)) => f(&self.others[i].mode, inner),
            _ => f(&self.outer, &mut s.outer),
        }
        true
    }
}
