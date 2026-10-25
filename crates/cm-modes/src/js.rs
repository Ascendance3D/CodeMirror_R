//! Small JavaScript-semantics helpers shared by the ported modes: string case
//! mapping, `\s` / `\w` classes and the string-or-null-or-undefined values that
//! modes pass around as styles. Everything works on UTF-16 code units, like JS.

pub use cm_core::text::{from_u16, is_js_space, is_js_word, to_u16};

/// JS `String.prototype.toLowerCase` on UTF-16 units. Lone surrogates pass through;
/// well-formed runs use Rust's `str::to_lowercase`, which follows the same Unicode
/// default case mapping (final sigma included).
pub fn to_lower(s: &[u16]) -> Vec<u16> {
    if s.iter().all(|&c| c < 0x80) {
        return s
            .iter()
            .map(|&c| {
                if (b'A' as u16..=b'Z' as u16).contains(&c) {
                    c + 32
                } else {
                    c
                }
            })
            .collect();
    }
    map_runs(s, |t| t.to_lowercase())
}

/// JS `String.prototype.toUpperCase` on UTF-16 units.
pub fn to_upper(s: &[u16]) -> Vec<u16> {
    map_runs(s, |t| t.to_uppercase())
}

fn map_runs(s: &[u16], f: impl Fn(&str) -> String) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    let mut run = String::new();
    for r in char::decode_utf16(s.iter().copied()) {
        match r {
            Ok(c) => run.push(c),
            Err(e) => {
                out.extend(f(&run).encode_utf16());
                run.clear();
                out.push(e.unpaired_surrogate());
            }
        }
    }
    out.extend(f(&run).encode_utf16());
    out
}

/// `a == b` for a UTF-16 string and a literal.
pub fn eq(a: &[u16], b: &str) -> bool {
    b.encode_utf16().eq(a.iter().copied())
}

/// `s.startsWith(prefix)`.
pub fn starts_with(s: &[u16], prefix: &str) -> bool {
    let mut it = s.iter();
    prefix.encode_utf16().all(|c| it.next() == Some(&c))
}

/// `/[\s ]*/` length from the start (`\s` already contains U+00A0).
pub fn space_len(s: &[u16]) -> usize {
    s.iter().take_while(|&&c| is_js_space(c)).count()
}

/// `/\S/.test(s)`.
pub fn has_non_space(s: &[u16]) -> bool {
    s.iter().any(|&c| !is_js_space(c))
}

/// `/\d/` (ASCII only in non-unicode JS regexps).
pub fn is_digit(c: u16) -> bool {
    (b'0' as u16..=b'9' as u16).contains(&c)
}

pub fn is_hex(c: u16) -> bool {
    is_digit(c)
        || (b'a' as u16..=b'f' as u16).contains(&c)
        || (b'A' as u16..=b'F' as u16).contains(&c)
}

/// One code unit as a char for `match` arms; non-ASCII units map to U+FFFF.
pub fn ch(c: u16) -> char {
    if c < 0x80 {
        c as u8 as char
    } else {
        '\u{ffff}'
    }
}

/// A numeric mode-state field that may hold `CodeMirror.Pass` instead of a number:
/// htmlmixed passes the html mode's `indent()` result — `Pass` inside `<pre>` — to an
/// inner mode's `startState(base)`, and `base || 0` keeps the Pass object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JsNum {
    Num(f64),
    Pass,
}

impl JsNum {
    /// `a || 0`.
    pub fn or_zero(a: crate::JsArg) -> JsNum {
        match a {
            crate::JsArg::Pass => JsNum::Pass,
            other => JsNum::Num(other.or_zero()),
        }
    }
    /// `self + x`. On Pass the JS result is the *string* `"CodeMirror.Pass" + x`;
    /// a number can not hold that, so it is NaN here (an editor treats both as "not
    /// a usable indentation").
    pub fn plus(self, x: f64) -> f64 {
        match self {
            JsNum::Num(n) => n + x,
            JsNum::Pass => f64::NAN,
        }
    }
    /// `self - x`: NaN on Pass, as in JS.
    pub fn minus(self, x: f64) -> f64 {
        match self {
            JsNum::Num(n) => n - x,
            JsNum::Pass => f64::NAN,
        }
    }
    /// The value returned as is from `indent()`: Pass stays Pass (`None`).
    pub fn value(self) -> Option<f64> {
        match self {
            JsNum::Num(n) => Some(n),
            JsNum::Pass => None,
        }
    }
}

/// A JS value that is a string, `null` or `undefined` — what tokenizers return as
/// styles and what modes sometimes concatenate (`null + " error"` is
/// `"null error"` in JS, and that string does reach the editor).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsStr {
    Undefined,
    Null,
    S(String),
}

impl JsStr {
    pub fn s(v: &str) -> JsStr {
        JsStr::S(v.to_string())
    }
    /// JS truthiness: `""`, `null`, `undefined` are falsy.
    pub fn truthy(&self) -> bool {
        matches!(self, JsStr::S(s) if !s.is_empty())
    }
    /// `self + suffix` with JS string conversion.
    pub fn concat(&self, suffix: &str) -> JsStr {
        JsStr::S(format!("{}{}", self.as_js_string(), suffix))
    }
    /// `String(self)`.
    pub fn as_js_string(&self) -> String {
        match self {
            JsStr::Undefined => "undefined".into(),
            JsStr::Null => "null".into(),
            JsStr::S(s) => s.clone(),
        }
    }
    /// `self == "lit"` (loose equality; null/undefined never equal a string).
    pub fn is(&self, lit: &str) -> bool {
        matches!(self, JsStr::S(s) if s == lit)
    }
    /// The style a mode returns: null and undefined both mean "no style".
    pub fn into_style(self) -> Option<String> {
        match self {
            JsStr::S(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_deref(&self) -> Option<&str> {
        match self {
            JsStr::S(s) => Some(s),
            _ => None,
        }
    }
}

impl From<Option<String>> for JsStr {
    fn from(v: Option<String>) -> JsStr {
        match v {
            Some(s) => JsStr::S(s),
            None => JsStr::Null,
        }
    }
}

impl From<&str> for JsStr {
    fn from(v: &str) -> JsStr {
        JsStr::S(v.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lower() {
        assert_eq!(from_u16(&to_lower(&to_u16("ABC dé"))), "abc dé");
        assert_eq!(from_u16(&to_lower(&to_u16("lin\u{212a}"))), "link");
        assert_eq!(to_lower(&[0xd800, 0x41]), vec![0xd800, 0x61]);
        assert_eq!(from_u16(&to_lower(&to_u16("ΟΔΟΣ"))), "οδος");
    }
}
