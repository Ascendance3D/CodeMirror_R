//! `src/util/StringStream.js`: the cursor modes tokenize with.
//!
//! Positions are UTF-16 code unit offsets. JS regexp arguments become closures
//! ([`StringStream::match_with`]); JS `eat(string | RegExp | function)` is split into
//! [`StringStream::eat`], [`StringStream::eat_str`] and [`StringStream::eat_fn`].

use std::cell::Cell;

use super::misc::count_column;
use super::regexp::{Match, RegExp};
use crate::text::{Text, index_of, is_js_space};

thread_local! {
    static NEXT_STREAM_ID: Cell<u64> = const { Cell::new(1) };
}

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
    id: u64,
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
            id: NEXT_STREAM_ID.with(|n| {
                let id = n.get();
                n.set(id + 1);
                id
            }),
        }
    }
    /// The object identity of this stream: distinct for every [`StringStream::new`]
    /// on a thread. `addon/mode/overlay.js` compares streams (`stream !=
    /// state.streamSeen`) to notice a new line.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// `pos` as the JS number: a position before the start of the line is stored
    /// in two's complement (see [`StringStream::back_up`]).
    fn ipos(&self) -> isize {
        self.pos as isize
    }
    /// `string.charAt(i)`: `None` where the JS returns `""` (out of range,
    /// negative included).
    fn char_at(&self, i: isize) -> Option<u16> {
        if i < 0 {
            None
        } else {
            self.string.get(i as usize).copied()
        }
    }
    /// The begin offset of `string.slice(i)` / `string.substr(i)`: a negative `i`
    /// counts from the end and clamps at 0.
    fn js_from(&self, i: isize) -> usize {
        let len = self.string.len() as isize;
        if i < 0 {
            (len + i).max(0) as usize
        } else {
            (i as usize).min(self.string.len())
        }
    }

    pub fn eol(&self) -> bool {
        self.ipos() >= self.string.len() as isize
    }
    pub fn sol(&self) -> bool {
        self.pos == self.line_start
    }
    /// `peek()`: `undefined` (None) at the end of the line or before its start.
    pub fn peek(&self) -> Option<u16> {
        self.char_at(self.ipos())
    }
    /// `next()`. Before the start of the line (after [`StringStream::back_up`] past
    /// it) the JS returns `""` and still advances `pos`; that is `None` here too, but
    /// `pos` moves as in CM5.
    pub fn next(&mut self) -> Option<u16> {
        if self.ipos() < self.string.len() as isize {
            let c = self.char_at(self.ipos());
            self.pos = self.pos.wrapping_add(1);
            c
        } else {
            None
        }
    }
    /// `eat(ch)` with a one-character string.
    pub fn eat(&mut self, ch: u16) -> Option<u16> {
        match self.peek() {
            Some(c) if c == ch => {
                self.pos = self.pos.wrapping_add(1);
                Some(c)
            }
            _ => None,
        }
    }
    /// `eat(string)`: compares the *single* next character to the whole string.
    /// Quirk kept from the JS: `eat("")` at the end of the line (or before its
    /// start) succeeds and moves `pos` on, because `charAt` returns `""` there.
    pub fn eat_str(&mut self, m: &str) -> bool {
        let m: Vec<u16> = m.encode_utf16().collect();
        let c = self.char_at(self.ipos());
        let ch: &[u16] = match &c {
            Some(c) => std::slice::from_ref(c),
            None => &[],
        };
        if ch == m.as_slice() {
            self.pos = self.pos.wrapping_add(1);
            true
        } else {
            false
        }
    }
    /// `eat(regexp | function)`: never called where `charAt` is `""` (`ch &&`).
    pub fn eat_fn<F: Fn(u16) -> bool>(&mut self, f: F) -> Option<u16> {
        match self.peek() {
            Some(c) if f(c) => {
                self.pos = self.pos.wrapping_add(1);
                Some(c)
            }
            _ => None,
        }
    }
    pub fn eat_while<F: Fn(u16) -> bool>(&mut self, f: F) -> bool {
        let start = self.ipos();
        while self.eat_fn(&f).is_some() {}
        self.ipos() > start
    }
    pub fn eat_while_char(&mut self, ch: u16) -> bool {
        let start = self.ipos();
        while self.eat(ch).is_some() {}
        self.ipos() > start
    }
    /// `eatSpace()`: `/[\s ]/`.
    pub fn eat_space(&mut self) -> bool {
        let start = self.ipos();
        while self.peek().is_some_and(is_js_space) {
            self.pos = self.pos.wrapping_add(1);
        }
        self.ipos() > start
    }
    pub fn skip_to_end(&mut self) {
        self.pos = self.string.len();
    }
    /// `skipTo(ch)`: `indexOf` from `pos` (works for multi-char strings too). A
    /// negative `pos` searches from 0, as `indexOf` does.
    pub fn skip_to(&mut self, ch: &[u16]) -> bool {
        match index_of(self.string, ch, self.ipos().max(0) as usize) {
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
    /// `backUp(n)`: `this.pos -= n`. CM5 does not stop at 0: `pos` (and a `start`
    /// copied from it) can go negative, and the readers then follow the JS string
    /// semantics (`charAt` is `""`, `slice`/`substr` count from the end). A negative
    /// position is stored in two's complement, so `pos as isize` is the JS value.
    pub fn back_up(&mut self, n: usize) {
        self.pos = self.pos.wrapping_sub(n);
    }
    pub fn column(&mut self) -> i64 {
        // `lastColumnPos` is never negative: it only takes `start` when `start` is
        // greater, so a negative `start` keeps the last value, as in the JS.
        if (self.last_column_pos as isize) < self.start as isize {
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
    /// `match(string, consume, caseInsensitive)`: compares
    /// `string.substr(pos, pattern.length)`.
    pub fn match_str(&mut self, pattern: &str, consume: bool, case_insensitive: bool) -> bool {
        let pat: Vec<u16> = pattern.encode_utf16().collect();
        let begin = self.js_from(self.ipos());
        let end = (begin + pat.len()).min(self.string.len());
        let substr = &self.string[begin..end];
        let eq = if case_insensitive {
            String::from_utf16_lossy(substr).to_lowercase()
                == String::from_utf16_lossy(&pat).to_lowercase()
        } else {
            substr == pat.as_slice()
        };
        if eq {
            if consume {
                self.pos = self.pos.wrapping_add(pat.len());
            }
            true
        } else {
            false
        }
    }
    /// `match(regexp, consume)`. `f` runs the "regexp" on the rest of the line
    /// (`string.slice(pos)`) and returns the match index and length. A match that
    /// does not start at the current position counts as no match (the JS returns
    /// null).
    pub fn match_with<F: FnOnce(&[u16]) -> Option<RegexMatch>>(
        &mut self,
        f: F,
        consume: bool,
    ) -> Option<RegexMatch> {
        let m = f(&self.string[self.js_from(self.ipos())..])?;
        if m.index > 0 {
            return None;
        }
        if consume {
            self.pos = self.pos.wrapping_add(m.len);
        }
        Some(m)
    }
    /// `match(regexp, consume)`: `this.string.slice(this.pos).match(regexp)`. The
    /// match spans are relative to `pos` (the sliced string). A match that does not
    /// start at `pos` is no match. With `g`, JS `String.prototype.match` returns
    /// every match and no `index`, so the first match is taken wherever it starts
    /// and its length consumed. The returned `Match` is that first match with all
    /// its capture groups: `group(1)` is capture 1, where the JS array's `[1]` is
    /// the second whole match.
    pub fn match_regexp(&mut self, re: &RegExp, consume: bool) -> Option<Match> {
        let rest = &self.string[self.js_from(self.ipos())..];
        let m = re.exec(rest)?;
        if re.flags().global {
            if consume {
                self.pos = self.pos.wrapping_add(m.len());
            }
            return Some(m);
        }
        if m.index() > 0 {
            return None;
        }
        if consume {
            self.pos = self.pos.wrapping_add(m.len());
        }
        Some(m)
    }
    /// `current()`: `string.slice(start, pos)`, with the JS `slice` rules for
    /// negative bounds (from the end) and an end before the begin (empty).
    pub fn current(&self) -> &'a [u16] {
        let begin = self.js_from(self.start as isize);
        let end = self.js_from(self.ipos()).max(begin);
        &self.string[begin..end]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::to_u16;
    use std::cell::RefCell;

    /// `match(regexp)`: without `g` only a match at `pos` counts and a failed match
    /// consumes nothing; with `g` the first match counts wherever it starts, its
    /// length is consumed, and `group(1)` is its capture 1 (JS gives `["k-"]`, whose
    /// `[1]` would be a second whole match; see `match_regexp`). Values from CM5
    /// `StringStream.match` in node.
    #[test]
    fn match_regexp_consumption() {
        use crate::util::regexp::RegExp;
        let s = to_u16("k--k-");
        let mut st = StringStream::new(&s, Some(4), None);
        let re = RegExp::new("([K])(-)", "iu").unwrap();
        let m = st.match_regexp(&re, true).unwrap();
        assert_eq!(
            (m.index(), m.len(), m.group(1), st.pos),
            (0, 2, Some((0, 1)), 2)
        );
        assert!(st.match_regexp(&re, true).is_none());
        assert_eq!(st.pos, 2);
        let m = st.match_regexp(&re, false).map(|m| m.len());
        assert_eq!((m, st.pos), (None, 2));
        let re = RegExp::new("([K])-", "giu").unwrap();
        let m = st.match_regexp(&re, true).unwrap();
        assert_eq!(
            (m.index(), m.len(), m.group(1), st.pos),
            (1, 2, Some((1, 2)), 4)
        );
    }

    /// `hideFirstChars(n, inner)` (`src/util/StringStream.js:75`; used by the pug and
    /// soy modes, not ported): `lineStart` is raised by `n` while `inner` runs, which
    /// moves `sol()`, `column()` and `indentation()`, then restored. Values confirmed
    /// against the JS in node.
    #[test]
    fn hide_first_chars_matches_cm5() {
        let s = to_u16("  \tfoo bar");
        let mut st = StringStream::new(&s, Some(4), None);
        st.pos = 3;
        st.start = 3;
        assert!(!st.sol());
        assert_eq!((st.column(), st.indentation()), (4, 4));
        let r = st.hide_first_chars(3, |st| {
            let outer = (st.sol(), st.column(), st.indentation(), st.line_start);
            let nested =
                st.hide_first_chars(1, |st| (st.line_start, st.column(), st.indentation()));
            (outer, nested)
        });
        assert_eq!(r, ((true, 0, 0, 3), (4, -1, -1)));
        assert!(!st.sol());
        assert_eq!((st.column(), st.indentation(), st.line_start), (4, 4, 0));

        st.pos = 7;
        st.start = 7;
        let r = st.hide_first_chars(3, |st| (st.sol(), st.column(), st.indentation()));
        assert_eq!(r, (false, 4, 0));
        assert_eq!(st.line_start, 0);
    }

    struct Recorder(RefCell<Vec<usize>>);
    impl LineOracle for Recorder {
        fn look_ahead(&self, _n: i64) -> Option<Text> {
            None
        }
        fn base_token(&self, pos: usize) -> Option<(Option<String>, i64)> {
            self.0.borrow_mut().push(pos);
            Some((Some("t".into()), 3 - pos as i64))
        }
    }

    /// `baseToken()` (`src/util/StringStream.js:84`): `oracle && oracle.baseToken(this.pos)`.
    /// Without an oracle the JS yields a falsy value (`None`); with one, the oracle
    /// gets the stream's current `pos` and its result is returned as is.
    #[test]
    fn base_token_forwards_pos_to_oracle() {
        let s = to_u16("abc");
        let st = StringStream::new(&s, Some(4), None);
        assert_eq!(st.base_token(), None);

        let rec = Recorder(RefCell::new(Vec::new()));
        let mut st = StringStream::new(&s, Some(4), Some(&rec));
        st.pos = 2;
        assert_eq!(st.base_token(), Some((Some("t".to_string()), 1)));
        assert_eq!(*rec.0.borrow(), vec![2]);
    }
}
