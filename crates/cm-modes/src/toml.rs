//! Port of `mode/toml/toml.js` (CodeMirror 5.65.21), quirks included: it is a
//! loose highlighter, not a TOML parser.
//!
//! The JS regexps are hand-coded over UTF-16 code units:
//! * `/^.[^\\\"\']*/` — `.` is any unit but `\n`, `\r`, U+2028, U+2029 (non-unicode
//!   regexp); the class matches any unit but backslash and both quotes.
//! * `/^\d\d\d\d[\d\-\:\.T]*Z/` — `\d` is ASCII only.
//! * `/^\-?\d+(?:\.\d+)?/`.

use std::rc::Rc;

use cm_core::util::string_stream::{RegexMatch, StringStream};

use crate::{JsArg, MimeTarget, Mode, Registry, State};

/// The JS state object `{inString, stringType, lhs, inArray}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TomlState {
    pub in_string: bool,
    /// The opening quote (a one-character string in JS, `""` initially).
    pub string_type: Option<u16>,
    pub lhs: bool,
    pub in_array: i64,
}

pub struct TomlMode;

const BACKSLASH: u16 = b'\\' as u16;
const DQUOTE: u16 = b'"' as u16;
const SQUOTE: u16 = b'\'' as u16;

fn is_digit(c: u16) -> bool {
    (b'0' as u16..=b'9' as u16).contains(&c)
}

/// `/^.[^\\\"\']*/`
fn re_string_chunk(s: &[u16]) -> Option<RegexMatch> {
    let first = *s.first()?;
    if matches!(first, 0x0a | 0x0d | 0x2028 | 0x2029) {
        return None;
    }
    let rest = s[1..]
        .iter()
        .take_while(|&&c| c != BACKSLASH && c != DQUOTE && c != SQUOTE)
        .count();
    Some(RegexMatch {
        index: 0,
        len: 1 + rest,
    })
}

/// `/^\d\d\d\d[\d\-\:\.T]*Z/`. The class never contains `Z`, so backtracking can
/// not help: after the maximal run the next unit must be `Z`.
fn re_date(s: &[u16]) -> Option<RegexMatch> {
    if s.len() < 4 || !s[..4].iter().all(|&c| is_digit(c)) {
        return None;
    }
    let run = s[4..]
        .iter()
        .take_while(|&&c| {
            is_digit(c)
                || c == b'-' as u16
                || c == b':' as u16
                || c == b'.' as u16
                || c == b'T' as u16
        })
        .count();
    (s.get(4 + run) == Some(&(b'Z' as u16))).then_some(RegexMatch {
        index: 0,
        len: 4 + run + 1,
    })
}

/// `/^\-?\d+(?:\.\d+)?/`
fn re_number(s: &[u16]) -> Option<RegexMatch> {
    let mut i = usize::from(s.first() == Some(&(b'-' as u16)));
    let digits = s[i..].iter().take_while(|&&c| is_digit(c)).count();
    if digits == 0 {
        return None;
    }
    i += digits;
    if s.get(i) == Some(&(b'.' as u16)) {
        let frac = s[i + 1..].iter().take_while(|&&c| is_digit(c)).count();
        if frac > 0 {
            i += 1 + frac;
        }
    }
    Some(RegexMatch { index: 0, len: i })
}

impl Mode for TomlMode {
    fn name(&self) -> &str {
        "toml"
    }

    fn start_state(&self, _a1: JsArg, _a2: JsArg) -> Option<State> {
        Some(State::new(TomlState {
            in_string: false,
            string_type: None,
            lhs: true,
            in_array: 0,
        }))
    }

    fn token(&self, stream: &mut StringStream<'_>, state: &mut State) -> Option<String> {
        let state = state.get_mut::<TomlState>();
        //check for state changes
        if !state.in_string && (stream.peek() == Some(DQUOTE) || stream.peek() == Some(SQUOTE)) {
            state.string_type = stream.peek();
            stream.next(); // Skip quote
            state.in_string = true; // Update state
        }
        if stream.sol() && state.in_array == 0 {
            state.lhs = true;
        }
        //return state
        if state.in_string {
            while state.in_string && !stream.eol() {
                if stream.peek() == state.string_type {
                    stream.next(); // Skip quote
                    state.in_string = false; // Clear flag
                } else if stream.peek() == Some(BACKSLASH) {
                    stream.next();
                    stream.next();
                } else if stream.match_with(re_string_chunk, true).is_none() {
                    // `.` does not match U+2028/U+2029, so the JS loop never ends
                    // here (the page hangs). Fail loudly instead of hanging.
                    panic!("toml mode: JS spins forever on a line separator inside a string");
                }
            }
            return Some(
                if state.lhs {
                    "property string"
                } else {
                    "string"
                }
                .into(),
            ); // Token style
        } else if state.in_array != 0 && stream.peek() == Some(b']' as u16) {
            stream.next();
            state.in_array -= 1;
            return Some("bracket".into());
        } else if state.lhs
            && stream.peek() == Some(b'[' as u16)
            && stream.skip_to_char(b']' as u16)
        {
            stream.next(); //skip closing ]
            // array of objects has an extra open & close []
            if stream.peek() == Some(b']' as u16) {
                stream.next();
            }
            return Some("atom".into());
        } else if stream.peek() == Some(b'#' as u16) {
            stream.skip_to_end();
            return Some("comment".into());
        } else if stream.eat_space() {
            return None;
        } else if state.lhs && stream.eat_while(|c| c != b'=' as u16 && c != b' ' as u16) {
            return Some("property".into());
        } else if state.lhs && stream.peek() == Some(b'=' as u16) {
            stream.next();
            state.lhs = false;
            return None;
        } else if !state.lhs && stream.match_with(re_date, true).is_some() {
            return Some("atom".into()); //date
        } else if !state.lhs
            && (stream.match_str("true", true, false) || stream.match_str("false", true, false))
        {
            return Some("atom".into());
        } else if !state.lhs && stream.peek() == Some(b'[' as u16) {
            state.in_array += 1;
            stream.next();
            return Some("bracket".into());
        } else if !state.lhs && stream.match_with(re_number, true).is_some() {
            return Some("number".into());
        } else if !stream.eat_space() {
            stream.next();
        }
        None
    }
}

/// `CodeMirror.defineMode("toml", ...)` and `defineMIME('text/x-toml', 'toml')`.
pub fn register(r: &mut Registry) {
    r.define_mode("toml", Rc::new(|_, _, _| Rc::new(TomlMode) as Rc<dyn Mode>));
    r.define_mime("text/x-toml", MimeTarget::Name("toml".into()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use cm_core::text::to_u16;

    #[test]
    fn regexes() {
        assert_eq!(re_number(&to_u16("-12.5x")).map(|m| m.len), Some(5));
        assert_eq!(re_number(&to_u16("12.x")).map(|m| m.len), Some(2));
        assert_eq!(re_number(&to_u16("-x")), None);
        assert_eq!(
            re_date(&to_u16("1979-05-27T07:32:00Z")).map(|m| m.len),
            Some(20)
        );
        assert_eq!(re_date(&to_u16("1979-05-27")), None);
        assert_eq!(re_string_chunk(&to_u16("ab\\c")).map(|m| m.len), Some(2));
        assert_eq!(re_string_chunk(&to_u16("\"ab")).map(|m| m.len), Some(3));
        assert_eq!(re_string_chunk(&[0x2028, 0x41]), None);
    }
}
