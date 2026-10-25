//! `src/util/StringStream.js`: the cursor modes tokenize with.
//!
//! Positions are UTF-16 code unit offsets. JS regexp arguments become closures
//! ([`StringStream::match_with`]); JS `eat(string | RegExp | function)` is split into
//! [`StringStream::eat`], [`StringStream::eat_str`] and [`StringStream::eat_fn`].

use super::misc::count_column;
use crate::text::{Text, index_of, is_js_space};

/// The `lineOracle` the stream consults for `lookAhead` / `baseToken`.
pub trait LineOracle {
    fn look_ahead(&self, n: i64) -> Option<Text>;
    /// Returns `(type, size)`; `None` where the JS returns null/undefined.
    fn base_token(&self, pos: usize) -> Option<(Option<String>, i64)>;
}

/// A regexp match result for [`StringStream::match_with`]: the match index within
/// the remaining string and the matched length, both in UTF-16 units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegexMatch {
    pub index: usize,
    pub len: usize,
}

pub struct StringStream<'a> {
    pub pos: usize,
    pub start: usize,
    pub string: &'a [u16],
    pub tab_size: i64,
    last_column_pos: usize,
    last_column_value: i64,
    pub line_start: usize,
    pub line_oracle: Option<&'a dyn LineOracle>,
}

impl<'a> StringStream<'a> {
    /// `new StringStream(string, tabSize, lineOracle)`; `tabSize` falls back to 8.
    pub fn new(
        string: &'a [u16],
        tab_size: Option<i64>,
        line_oracle: Option<&'a dyn LineOracle>,
    ) -> Self {
        StringStream {
            pos: 0,
            start: 0,
            string,
            tab_size: match tab_size {
                Some(t) if t != 0 => t,
                _ => 8,
            },
            last_column_pos: 0,
            last_column_value: 0,
            line_start: 0,
            line_oracle,
        }
    }

    pub fn eol(&self) -> bool {
        self.pos >= self.string.len()
    }
    pub fn sol(&self) -> bool {
        self.pos == self.line_start
    }
    /// `peek()`: `undefined` (None) at the end of the line.
    pub fn peek(&self) -> Option<u16> {
        self.string.get(self.pos).copied()
    }
    pub fn next(&mut self) -> Option<u16> {
        if self.pos < self.string.len() {
            self.pos += 1;
            Some(self.string[self.pos - 1])
        } else {
            None
        }
    }
    /// `eat(ch)` with a one-character string.
    pub fn eat(&mut self, ch: u16) -> Option<u16> {
        match self.peek() {
            Some(c) if c == ch => {
                self.pos += 1;
                Some(c)
            }
            _ => None,
        }
    }
    /// `eat(string)`: compares the *single* next character to the whole string.
    /// Quirk kept from the JS: `eat("")` at the end of the line succeeds and moves
    /// `pos` past the end, because `charAt` returns `""` there.
    pub fn eat_str(&mut self, m: &str) -> bool {
        let m: Vec<u16> = m.encode_utf16().collect();
        let ch: &[u16] = match self.string.get(self.pos) {
            Some(c) => std::slice::from_ref(c),
            None => &[],
        };
        if ch == m.as_slice() {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    /// `eat(regexp | function)`: never called at the end of the line (`ch &&`).
    pub fn eat_fn<F: Fn(u16) -> bool>(&mut self, f: F) -> Option<u16> {
        match self.peek() {
            Some(c) if f(c) => {
                self.pos += 1;
                Some(c)
            }
            _ => None,
        }
    }
    pub fn eat_while<F: Fn(u16) -> bool>(&mut self, f: F) -> bool {
        let start = self.pos;
        while self.eat_fn(&f).is_some() {}
        self.pos > start
    }
    pub fn eat_while_char(&mut self, ch: u16) -> bool {
        let start = self.pos;
        while self.eat(ch).is_some() {}
        self.pos > start
    }
    /// `eatSpace()`: `/[\s ]/`.
    pub fn eat_space(&mut self) -> bool {
        let start = self.pos;
        while self.peek().is_some_and(is_js_space) {
            self.pos += 1;
        }
        self.pos > start
    }
    pub fn skip_to_end(&mut self) {
        self.pos = self.string.len();
    }
    /// `skipTo(ch)`: `indexOf` from `pos` (works for multi-char strings too).
    pub fn skip_to(&mut self, ch: &[u16]) -> bool {
        match index_of(self.string, ch, self.pos) {
            Some(found) => {
                self.pos = found;
                true
            }
            None => false,
        }
    }
    pub fn skip_to_char(&mut self, ch: u16) -> bool {
        self.skip_to(&[ch])
    }
    pub fn back_up(&mut self, n: usize) {
        self.pos -= n;
    }
    pub fn column(&mut self) -> i64 {
        if self.last_column_pos < self.start {
            self.last_column_value = count_column(
                self.string,
                Some(self.start),
                self.tab_size,
                self.last_column_pos,
                self.last_column_value,
            );
            self.last_column_pos = self.start;
        }
        self.last_column_value
            - if self.line_start != 0 {
                count_column(self.string, Some(self.line_start), self.tab_size, 0, 0)
            } else {
                0
            }
    }
    pub fn indentation(&self) -> i64 {
        count_column(self.string, None, self.tab_size, 0, 0)
            - if self.line_start != 0 {
                count_column(self.string, Some(self.line_start), self.tab_size, 0, 0)
            } else {
                0
            }
    }
    /// `match(string, consume, caseInsensitive)`.
    pub fn match_str(&mut self, pattern: &str, consume: bool, case_insensitive: bool) -> bool {
        let pat: Vec<u16> = pattern.encode_utf16().collect();
        let end = (self.pos + pat.len()).min(self.string.len());
        let substr = &self.string[self.pos.min(end)..end];
        let eq = if case_insensitive {
            String::from_utf16_lossy(substr).to_lowercase()
                == String::from_utf16_lossy(&pat).to_lowercase()
        } else {
            substr == pat.as_slice()
        };
        if eq {
            if consume {
                self.pos += pat.len();
            }
            true
        } else {
            false
        }
    }
    /// `match(regexp, consume)`. `f` runs the "regexp" on the rest of the line and
    /// returns the match index and length. A match that does not start at the current
    /// position counts as no match (the JS returns null).
    pub fn match_with<F: FnOnce(&[u16]) -> Option<RegexMatch>>(
        &mut self,
        f: F,
        consume: bool,
    ) -> Option<RegexMatch> {
        let m = f(&self.string[self.pos.min(self.string.len())..])?;
        if m.index > 0 {
            return None;
        }
        if consume {
            self.pos += m.len;
        }
        Some(m)
    }
    pub fn current(&self) -> &'a [u16] {
        &self.string[self.start..self.pos.min(self.string.len()).max(self.start)]
    }
    pub fn current_string(&self) -> String {
        String::from_utf16_lossy(self.current())
    }
    pub fn hide_first_chars<R, F: FnOnce(&mut Self) -> R>(&mut self, n: usize, inner: F) -> R {
        self.line_start += n;
        // The JS restores lineStart in a `finally`; Rust has no exceptions to unwind.
        let r = inner(self);
        self.line_start -= n;
        r
    }
    pub fn look_ahead(&self, n: i64) -> Option<Text> {
        self.line_oracle.and_then(|o| o.look_ahead(n))
    }
    pub fn base_token(&self) -> Option<(Option<String>, i64)> {
        self.line_oracle.and_then(|o| o.base_token(self.pos))
    }
}
