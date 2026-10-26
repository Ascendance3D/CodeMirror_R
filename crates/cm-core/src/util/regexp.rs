//! JavaScript `RegExp` (ECMAScript 2018 with Annex B) on UTF-16 code units.
//!
//! CodeMirror 5 modes are full of regexps: `stream.match(/^.../)`, the rule tables of
//! `addon/mode/simple.js`, the `file` patterns of `mode/meta.js`, the delimiters of
//! `addon/mode/multiplex.js`. This is a backtracking matcher with the JS semantics
//! those need, run on the same UTF-16 strings the modes see, so match indexes and
//! lengths are the JS ones:
//!
//! - disjunction, groups (capturing, `(?:)`, named `(?<n>)`), backreferences
//!   (`\1`, `\k<n>`), greedy and lazy quantifiers, `{n,m}`;
//! - lookahead `(?=)` / `(?!)` (quantifiable without `u`, Annex B) and lookbehind
//!   `(?<=)` / `(?<!)`;
//! - `^ $ \b \B`, `.`, classes with ranges and `\d \w \s` (and negations);
//! - flags `g i m s u y`. `i` uses the spec's `Canonicalize` (`toUpperCase` of one
//!   code unit, never mapping non-ASCII to ASCII); with `u` it uses simple lowercase
//!   mapping, an approximation of Unicode simple case folding.
//! - Annex B leniency without `u`: a lone `{`, `}` or `]` is literal, `\8` / `\9`
//!   and unknown escapes are identity escapes, a `\N` above the group count is a
//!   legacy octal escape, `\c` without a letter is a literal backslash.
//!
//! Not modeled: `lastIndex` state (callers pass the start explicitly, see
//! [`RegExp::exec_from`]); Unicode property escapes `\p{..}`; the `v` flag. A
//! lookbehind is matched forward from each earlier start (nearest first), which
//! gives the JS result except for captures inside it that depend on right-to-left
//! greediness. Each quantified iteration of a complex atom recurses, so a group
//! repeated tens of thousands of times on one line can exhaust the stack; single
//! characters, `.` and classes repeat without recursion.

use crate::text::{is_js_space, is_js_word};

/// A pattern the JS `RegExp` constructor rejects (`SyntaxError`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegExpError(pub String);

impl std::fmt::Display for RegExpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Invalid regular expression: {}", self.0)
    }
}

impl std::error::Error for RegExpError {}

/// `RegExp.prototype.flags`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    pub global: bool,
    pub ignore_case: bool,
    pub multiline: bool,
    pub dot_all: bool,
    pub unicode: bool,
    pub sticky: bool,
}

impl Flags {
    fn parse(s: &str) -> Result<Flags, RegExpError> {
        let mut f = Flags::default();
        for c in s.chars() {
            let slot = match c {
                'g' => &mut f.global,
                'i' => &mut f.ignore_case,
                'm' => &mut f.multiline,
                's' => &mut f.dot_all,
                'u' => &mut f.unicode,
                'y' => &mut f.sticky,
                _ => return Err(RegExpError(format!("Invalid flags '{s}'"))),
            };
            if *slot {
                return Err(RegExpError(format!("Invalid flags '{s}'")));
            }
            *slot = true;
        }
        Ok(f)
    }
    /// The flags string in the order `RegExp.prototype.flags` gives.
    pub fn as_string(&self) -> String {
        let mut s = String::new();
        for (on, c) in [
            (self.global, 'g'),
            (self.ignore_case, 'i'),
            (self.multiline, 'm'),
            (self.dot_all, 's'),
            (self.unicode, 'u'),
            (self.sticky, 'y'),
        ] {
            if on {
                s.push(c);
            }
        }
        s
    }
}

#[derive(Clone, Debug)]
enum ClassItem {
    Range(u32, u32),
    Digit(bool),
    Word(bool),
    Space(bool),
}

#[derive(Clone, Debug)]
enum Node {
    Empty,
    /// A literal (canonicalized under `i`).
    Char(u32),
    Any,
    Class {
        items: Vec<ClassItem>,
        negate: bool,
    },
    Start,
    End,
    /// `\b` (false) or `\B` (true).
    WordBoundary(bool),
    Group(Box<Node>, Option<usize>),
    Look {
        node: Box<Node>,
        behind: bool,
        negate: bool,
    },
    BackRef(usize),
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    Repeat(Box<Repeat>),
}

#[derive(Clone, Debug)]
struct Repeat {
    node: Node,
    min: u32,
    max: Option<u32>,
    greedy: bool,
    /// The capture groups inside `node`: JS clears them before each iteration.
    first_group: usize,
    group_count: usize,
}

/// A compiled JS regexp.
#[derive(Clone, Debug)]
pub struct RegExp {
    source: Vec<u16>,
    flags: Flags,
    node: Node,
    /// Capture groups, group 0 excluded.
    groups: usize,
    names: Vec<(String, usize)>,
}

/// A successful match: `[start, end)` of the whole match (group 0) and of each
/// capture group, in UTF-16 units of the searched string. `None` = undefined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    caps: Vec<Option<(usize, usize)>>,
}

impl Match {
    /// `match.index`.
    pub fn index(&self) -> usize {
        self.caps[0].unwrap().0
    }
    pub fn end(&self) -> usize {
        self.caps[0].unwrap().1
    }
    /// `match[0].length`.
    pub fn len(&self) -> usize {
        self.end() - self.index()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// `match.length`: the group count plus one.
    pub fn count(&self) -> usize {
        self.caps.len()
    }
    /// The span of `match[i]`; `None` when the group did not participate.
    pub fn group(&self, i: usize) -> Option<(usize, usize)> {
        self.caps.get(i).copied().flatten()
    }
    /// The text of `match[i]` within `s` (the string that was searched).
    pub fn text<'s>(&self, s: &'s [u16], i: usize) -> Option<&'s [u16]> {
        self.group(i).map(|(a, b)| &s[a..b])
    }
    /// Shifts every span by `d`, for a match made on `s.slice(d)`.
    pub fn offset(mut self, d: usize) -> Match {
        for (a, b) in self.caps.iter_mut().flatten() {
            *a += d;
            *b += d;
        }
        self
    }
}

const LINE_TERMINATORS: [u32; 4] = [0x0a, 0x0d, 0x2028, 0x2029];

fn is_line_terminator(c: u32) -> bool {
    LINE_TERMINATORS.contains(&c)
}

fn is_digit(c: u32) -> bool {
    (0x30..=0x39).contains(&c)
}

fn is_word_cp(c: u32) -> bool {
    c < 0x80 && is_js_word(c as u16)
}

fn is_space_cp(c: u32) -> bool {
    c <= 0xffff && is_js_space(c as u16)
}

fn hex_val(c: u32) -> Option<u32> {
    char::from_u32(c).and_then(|ch| ch.to_digit(16))
}

/// The spec's `Canonicalize(rer, ch)` for an `i` regexp: upper case without `u`,
/// (approximately) case-folded, so lower case, with `u`.
fn canonicalize(c: u32, unicode: bool) -> u32 {
    if c < 0x80 {
        return match (unicode, c) {
            (false, 0x61..=0x7a) => c - 32,
            (true, 0x41..=0x5a) => c + 32,
            _ => c,
        };
    }
    let Some(ch) = char::from_u32(c) else {
        return c; // a lone surrogate
    };
    if unicode {
        let mut l = ch.to_lowercase();
        return match (l.next(), l.next()) {
            (Some(x), None) => x as u32,
            _ => c,
        };
    }
    let mut u = ch.to_uppercase();
    match (u.next(), u.next()) {
        (Some(x), None) if (x as u32) <= 0xffff => {
            if (x as u32) < 0x80 {
                c
            } else {
                x as u32
            }
        }
        _ => c,
    }
}

fn lower_one(c: u32) -> u32 {
    char::from_u32(c)
        .map(|ch| {
            let mut l = ch.to_lowercase();
            match (l.next(), l.next()) {
                (Some(x), None) => x as u32,
                _ => c,
            }
        })
        .unwrap_or(c)
}

// --------------------------------------------------------------------------------
// Parser

struct Parser<'a> {
    s: &'a [u32],
    i: usize,
    flags: Flags,
    /// Total capture groups (a pre-scan; Annex B decides `\N` with it).
    total_groups: usize,
    names: Vec<(String, usize)>,
    next_group: usize,
}

const SYNTAX_CHARS: &str = "^$\\.*+?()[]{}|/";

impl<'a> Parser<'a> {
    fn err<T>(&self, msg: &str) -> Result<T, RegExpError> {
        Err(RegExpError(msg.to_string()))
    }
    fn peek(&self) -> Option<u32> {
        self.s.get(self.i).copied()
    }
    fn peek_at(&self, k: usize) -> Option<u32> {
        self.s.get(self.i + k).copied()
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c as u32) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn has_named_groups(&self) -> bool {
        !self.names.is_empty()
    }

    /// Counts the capture groups and collects the group names.
    fn prescan(s: &[u32]) -> Result<(usize, Vec<(String, usize)>), RegExpError> {
        let mut n = 0;
        let mut names = Vec::new();
        let mut i = 0;
        let mut in_class = false;
        while i < s.len() {
            let c = s[i];
            if c == '\\' as u32 {
                i += 2;
                continue;
            }
            if in_class {
                if c == ']' as u32 {
                    in_class = false;
                }
            } else if c == '[' as u32 {
                in_class = true;
            } else if c == '(' as u32 {
                if s.get(i + 1) != Some(&('?' as u32)) {
                    n += 1;
                } else if s.get(i + 2) == Some(&('<' as u32))
                    && !matches!(s.get(i + 3), Some(&x) if x == '=' as u32 || x == '!' as u32)
                {
                    n += 1;
                    let mut j = i + 3;
                    let mut name = String::new();
                    while j < s.len() && s[j] != '>' as u32 {
                        name.push(char::from_u32(s[j]).unwrap_or('\u{fffd}'));
                        j += 1;
                    }
                    if names.iter().any(|(m, _)| *m == name) {
                        return Err(RegExpError("Duplicate capture group name".into()));
                    }
                    names.push((name, n));
                }
            }
            i += 1;
        }
        Ok((n, names))
    }

    fn parse_disjunction(&mut self) -> Result<Node, RegExpError> {
        let mut alts = vec![self.parse_alternative()?];
        while self.eat('|') {
            alts.push(self.parse_alternative()?);
        }
        Ok(if alts.len() == 1 {
            alts.pop().unwrap()
        } else {
            Node::Alt(alts)
        })
    }

    fn parse_alternative(&mut self) -> Result<Node, RegExpError> {
        let mut seq = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' as u32 || c == ')' as u32 {
                break;
            }
            seq.push(self.parse_term()?);
        }
        Ok(match seq.len() {
            0 => Node::Empty,
            1 => seq.pop().unwrap(),
            _ => Node::Seq(seq),
        })
    }

    fn parse_term(&mut self) -> Result<Node, RegExpError> {
        let c = self.peek().unwrap();
        let group_before = self.next_group;
        // Assertions.
        if c == '^' as u32 {
            self.i += 1;
            return self.no_quantifier(Node::Start);
        }
        if c == '$' as u32 {
            self.i += 1;
            return self.no_quantifier(Node::End);
        }
        if c == '\\' as u32
            && matches!(self.peek_at(1), Some(x) if x == 'b' as u32 || x == 'B' as u32)
        {
            let neg = self.peek_at(1) == Some('B' as u32);
            self.i += 2;
            return self.no_quantifier(Node::WordBoundary(neg));
        }
        if c == '(' as u32 && self.peek_at(1) == Some('?' as u32) {
            let (behind, negate, len) = match (self.peek_at(2), self.peek_at(3)) {
                (Some(x), _) if x == '=' as u32 => (Some(false), false, 3),
                (Some(x), _) if x == '!' as u32 => (Some(false), true, 3),
                (Some(x), Some(y)) if x == '<' as u32 && y == '=' as u32 => (Some(true), false, 4),
                (Some(x), Some(y)) if x == '<' as u32 && y == '!' as u32 => (Some(true), true, 4),
                _ => (None, false, 0),
            };
            if let Some(behind) = behind {
                self.i += len;
                let inner = self.parse_disjunction()?;
                if !self.eat(')') {
                    return self.err("Unterminated group");
                }
                let node = Node::Look {
                    node: Box::new(inner),
                    behind,
                    negate,
                };
                // Annex B: a lookahead is quantifiable without `u`.
                if behind || self.flags.unicode {
                    return self.no_quantifier(node);
                }
                return self.quantified(node, group_before);
            }
        }
        let atom = self.parse_atom()?;
        self.quantified(atom, group_before)
    }

    fn no_quantifier(&mut self, node: Node) -> Result<Node, RegExpError> {
        if self.at_quantifier().is_some() {
            return self.err("Nothing to repeat");
        }
        Ok(node)
    }

    /// The quantifier at the cursor: `(min, max, length)`.
    fn at_quantifier(&self) -> Option<(u32, Option<u32>, usize)> {
        let c = self.peek()?;
        match char::from_u32(c)? {
            '*' => Some((0, None, 1)),
            '+' => Some((1, None, 1)),
            '?' => Some((0, Some(1), 1)),
            '{' => {
                let mut j = self.i + 1;
                let num = |j: &mut usize| -> Option<u32> {
                    let st = *j;
                    let mut v: u64 = 0;
                    while *j < self.s.len() && is_digit(self.s[*j]) {
                        v = (v * 10 + (self.s[*j] - 0x30) as u64).min(u32::MAX as u64);
                        *j += 1;
                    }
                    (*j > st).then_some(v as u32)
                };
                let min = num(&mut j)?;
                let max;
                if self.s.get(j) == Some(&('}' as u32)) {
                    max = Some(min);
                } else if self.s.get(j) == Some(&(',' as u32)) {
                    j += 1;
                    max = num(&mut j);
                    if self.s.get(j) != Some(&('}' as u32)) {
                        return None;
                    }
                } else {
                    return None;
                }
                Some((min, max, j + 1 - self.i))
            }
            _ => None,
        }
    }

    fn quantified(&mut self, atom: Node, group_before: usize) -> Result<Node, RegExpError> {
        let Some((min, max, len)) = self.at_quantifier() else {
            return Ok(atom);
        };
        if let Some(m) = max
            && m < min
        {
            return self.err("numbers out of order in {} quantifier");
        }
        self.i += len;
        let greedy = !self.eat('?');
        Ok(Node::Repeat(Box::new(Repeat {
            node: atom,
            min,
            max,
            greedy,
            first_group: group_before + 1,
            group_count: self.next_group - group_before,
        })))
    }

    fn parse_atom(&mut self) -> Result<Node, RegExpError> {
        let c = self.peek().unwrap();
        let u = self.flags.unicode;
        match char::from_u32(c) {
            Some('.') => {
                self.i += 1;
                Ok(Node::Any)
            }
            Some('(') => {
                self.i += 1;
                let mut index = None;
                if self.eat('?') {
                    if self.eat(':') {
                    } else if self.eat('<') {
                        while self.peek().is_some_and(|c| c != '>' as u32) {
                            self.i += 1;
                        }
                        if !self.eat('>') {
                            return self.err("Invalid capture group name");
                        }
                        self.next_group += 1;
                        index = Some(self.next_group);
                    } else {
                        return self.err("Invalid group");
                    }
                } else {
                    self.next_group += 1;
                    index = Some(self.next_group);
                }
                let inner = self.parse_disjunction()?;
                if !self.eat(')') {
                    return self.err("Unterminated group");
                }
                Ok(Node::Group(Box::new(inner), index))
            }
            Some('[') => self.parse_class(),
            Some('\\') => self.parse_atom_escape(),
            Some('*' | '+' | '?') => self.err("Nothing to repeat"),
            Some('{') if u || self.at_quantifier().is_some() => self.err("Nothing to repeat"),
            Some('}' | ']') if u => self.err("Lone quantifier brackets"),
            Some(')') => self.err("Unmatched ')'"),
            _ => {
                self.i += 1;
                Ok(self.literal(c))
            }
        }
    }

    fn literal(&self, c: u32) -> Node {
        Node::Char(if self.flags.ignore_case {
            canonicalize(c, self.flags.unicode)
        } else {
            c
        })
    }

    fn parse_atom_escape(&mut self) -> Result<Node, RegExpError> {
        self.i += 1; // `\`
        let Some(c) = self.peek() else {
            return self.err("\\ at end of pattern");
        };
        let u = self.flags.unicode;
        // Backreferences and legacy octal.
        if (0x31..=0x39).contains(&c) {
            let st = self.i;
            let mut v: u64 = 0;
            while let Some(d) = self.peek().filter(|&d| is_digit(d)) {
                v = (v * 10 + (d - 0x30) as u64).min(u32::MAX as u64);
                self.i += 1;
            }
            if v as usize <= self.total_groups {
                return Ok(Node::BackRef(v as usize));
            }
            if u {
                return self.err("Invalid escape");
            }
            self.i = st;
            if c >= 0x38 {
                self.i += 1;
                return Ok(self.literal(c)); // `\8`, `\9`
            }
            let v = self.legacy_octal();
            return Ok(self.literal(v));
        }
        if c == 0x30 {
            if !u && self.peek_at(1).is_some_and(is_digit) {
                let v = self.legacy_octal();
                return Ok(self.literal(v));
            }
            if u && self.peek_at(1).is_some_and(is_digit) {
                return self.err("Invalid decimal escape");
            }
            self.i += 1;
            return Ok(self.literal(0));
        }
        if let Some(item) = self.class_escape(c) {
            self.i += 1;
            return Ok(Node::Class {
                items: vec![item],
                negate: false,
            });
        }
        if c == 'k' as u32 && (u || self.has_named_groups()) {
            self.i += 1;
            if !self.eat('<') {
                return self.err("Invalid named reference");
            }
            let mut name = String::new();
            while let Some(c) = self.peek().filter(|&c| c != '>' as u32) {
                name.push(char::from_u32(c).unwrap_or('\u{fffd}'));
                self.i += 1;
            }
            if !self.eat('>') {
                return self.err("Invalid named reference");
            }
            return match self.names.iter().find(|(n, _)| *n == name) {
                Some(&(_, i)) => Ok(Node::BackRef(i)),
                None => self.err("Invalid named capture referenced"),
            };
        }
        let v = self.character_escape(false)?;
        Ok(self.literal(v))
    }

    /// Up to three octal digits, at most `\377`.
    fn legacy_octal(&mut self) -> u32 {
        let mut v = 0;
        let mut n = 0;
        while n < 3 {
            match self.peek() {
                Some(d) if (0x30..=0x37).contains(&d) && v * 8 + (d - 0x30) <= 0o377 => {
                    v = v * 8 + (d - 0x30);
                    self.i += 1;
                    n += 1;
                }
                _ => break,
            }
        }
        v
    }

    fn class_escape(&self, c: u32) -> Option<ClassItem> {
        Some(match char::from_u32(c)? {
            'd' => ClassItem::Digit(false),
            'D' => ClassItem::Digit(true),
            'w' => ClassItem::Word(false),
            'W' => ClassItem::Word(true),
            's' => ClassItem::Space(false),
            'S' => ClassItem::Space(true),
            _ => return None,
        })
    }

    /// A `CharacterEscape` after the `\` (the cursor is on the escape letter).
    /// `in_class`: Annex B's `\c` with a digit or `_` inside a class.
    fn character_escape(&mut self, in_class: bool) -> Result<u32, RegExpError> {
        let c = self.peek().unwrap();
        let u = self.flags.unicode;
        let ch = char::from_u32(c).unwrap_or('\u{fffd}');
        let simple = match ch {
            't' => Some(0x09),
            'n' => Some(0x0a),
            'v' => Some(0x0b),
            'f' => Some(0x0c),
            'r' => Some(0x0d),
            _ => None,
        };
        if let Some(v) = simple {
            self.i += 1;
            return Ok(v);
        }
        match ch {
            'c' => {
                let next = self.peek_at(1);
                if let Some(l) =
                    next.filter(|&l| char::from_u32(l).is_some_and(|x| x.is_ascii_alphabetic()))
                {
                    self.i += 2;
                    return Ok(l % 32);
                }
                if in_class
                    && !u
                    && let Some(l) = next.filter(|&l| is_digit(l) || l == '_' as u32)
                {
                    self.i += 2;
                    return Ok(l % 32);
                }
                if u {
                    return self.err("Invalid unicode escape");
                }
                // `\` is a literal; `c` is read next.
                Ok('\\' as u32)
            }
            'x' => {
                if let (Some(a), Some(b)) = (
                    self.peek_at(1).and_then(hex_val),
                    self.peek_at(2).and_then(hex_val),
                ) {
                    self.i += 3;
                    return Ok(a * 16 + b);
                }
                if u {
                    return self.err("Invalid escape");
                }
                self.i += 1;
                Ok(c)
            }
            'u' => {
                if u && self.peek_at(1) == Some('{' as u32) {
                    let mut j = self.i + 2;
                    let mut v: u32 = 0;
                    let st = j;
                    while let Some(h) = self.s.get(j).copied().and_then(hex_val) {
                        v = v.saturating_mul(16).saturating_add(h);
                        j += 1;
                    }
                    if j == st || self.s.get(j) != Some(&('}' as u32)) || v > 0x10ffff {
                        return self.err("Invalid Unicode escape");
                    }
                    self.i = j + 1;
                    return Ok(v);
                }
                if let Some(v) = self.hex4(1) {
                    self.i += 5;
                    if u && (0xd800..0xdc00).contains(&v)
                        && self.peek() == Some('\\' as u32)
                        && self.peek_at(1) == Some('u' as u32)
                        && let Some(lo) = self.hex4(2).filter(|lo| (0xdc00..0xe000).contains(lo))
                    {
                        self.i += 6;
                        return Ok(0x10000 + ((v - 0xd800) << 10) + (lo - 0xdc00));
                    }
                    return Ok(v);
                }
                if u {
                    return self.err("Invalid Unicode escape");
                }
                self.i += 1;
                Ok(c)
            }
            _ => {
                if u && !(SYNTAX_CHARS.contains(ch) || (in_class && ch == '-')) {
                    return self.err("Invalid escape");
                }
                self.i += 1;
                Ok(c)
            }
        }
    }

    fn hex4(&self, at: usize) -> Option<u32> {
        let mut v = 0;
        for k in 0..4 {
            v = v * 16 + self.peek_at(at + k).and_then(hex_val)?;
        }
        Some(v)
    }

    fn parse_class(&mut self) -> Result<Node, RegExpError> {
        self.i += 1; // `[`
        let negate = self.eat('^');
        let mut items = Vec::new();
        loop {
            let Some(c) = self.peek() else {
                return self.err("Unterminated character class");
            };
            if c == ']' as u32 {
                self.i += 1;
                break;
            }
            let a = self.class_atom()?;
            if self.peek() == Some('-' as u32) && self.peek_at(1).is_some_and(|c| c != ']' as u32) {
                self.i += 1;
                let b = self.class_atom()?;
                match (&a, &b) {
                    (ClassItem::Range(x, _), ClassItem::Range(y, _)) => {
                        if x > y {
                            return self.err("Range out of order in character class");
                        }
                        items.push(ClassItem::Range(*x, *y));
                    }
                    _ => {
                        if self.flags.unicode {
                            return self.err("Invalid character class");
                        }
                        // Annex B: a class escape at either end makes `-` literal.
                        items.push(a);
                        items.push(ClassItem::Range('-' as u32, '-' as u32));
                        items.push(b);
                    }
                }
            } else {
                items.push(a);
            }
        }
        Ok(Node::Class { items, negate })
    }

    fn class_atom(&mut self) -> Result<ClassItem, RegExpError> {
        let c = self.peek().unwrap();
        if c != '\\' as u32 {
            self.i += 1;
            return Ok(ClassItem::Range(c, c));
        }
        self.i += 1;
        let Some(e) = self.peek() else {
            return self.err("\\ at end of pattern");
        };
        if let Some(item) = self.class_escape(e) {
            self.i += 1;
            return Ok(item);
        }
        let u = self.flags.unicode;
        let v = match char::from_u32(e) {
            Some('b') => {
                self.i += 1;
                0x08
            }
            Some('-') if u => {
                self.i += 1;
                '-' as u32
            }
            Some('0') if u && !self.peek_at(1).is_some_and(is_digit) => {
                self.i += 1;
                0
            }
            Some('0'..='7') if !u => self.legacy_octal(),
            Some('8' | '9') if !u => {
                self.i += 1;
                e
            }
            Some('k') if !u => {
                self.i += 1;
                e
            }
            _ => self.character_escape(true)?,
        };
        Ok(ClassItem::Range(v, v))
    }
}

impl RegExp {
    /// `new RegExp(source, flags)`.
    pub fn new(source: &str, flags: &str) -> Result<RegExp, RegExpError> {
        RegExp::from_utf16(&source.encode_utf16().collect::<Vec<_>>(), flags)
    }

    /// `new RegExp(source, flags)` with a UTF-16 source.
    pub fn from_utf16(source: &[u16], flags: &str) -> Result<RegExp, RegExpError> {
        let flags = Flags::parse(flags)?;
        let units: Vec<u32> = if flags.unicode {
            char::decode_utf16(source.iter().copied())
                .zip(utf16_starts(source))
                .map(|(r, i)| r.map(|c| c as u32).unwrap_or(source[i] as u32))
                .collect()
        } else {
            source.iter().map(|&c| c as u32).collect()
        };
        let (total_groups, names) = Parser::prescan(&units)?;
        let mut p = Parser {
            s: &units,
            i: 0,
            flags,
            total_groups,
            names,
            next_group: 0,
        };
        let node = p.parse_disjunction()?;
        if p.i < units.len() {
            return p.err("Unmatched ')'");
        }
        Ok(RegExp {
            source: source.to_vec(),
            flags,
            node,
            groups: p.next_group,
            names: p.names,
        })
    }

    /// `regexp.source` as written (not re-escaped).
    pub fn source(&self) -> &[u16] {
        &self.source
    }
    pub fn source_string(&self) -> String {
        String::from_utf16_lossy(&self.source)
    }
    pub fn flags(&self) -> Flags {
        self.flags
    }
    pub fn ignore_case(&self) -> bool {
        self.flags.ignore_case
    }
    pub fn unicode(&self) -> bool {
        self.flags.unicode
    }
    /// The number of capture groups (`match.length - 1`).
    pub fn group_count(&self) -> usize {
        self.groups
    }
    /// The index of the named group `name`.
    pub fn group_index(&self, name: &str) -> Option<usize> {
        self.names.iter().find(|(n, _)| n == name).map(|&(_, i)| i)
    }

    /// One match attempt that must start at `pos`.
    pub fn match_at(&self, s: &[u16], pos: usize) -> Option<Match> {
        if pos > s.len() {
            return None;
        }
        let m = Matcher { re: self, s };
        let mut caps = vec![None; self.groups + 1];
        let mut end = None;
        if m.m(&self.node, pos, &mut caps, &mut |p, _| {
            end = Some(p);
            true
        }) {
            caps[0] = Some((pos, end.unwrap()));
            Some(Match { caps })
        } else {
            None
        }
    }

    /// `RegExpBuiltinExec` with `lastIndex = from` for a `g` or `y` regexp: the
    /// first match at or after `from` (only at `from` when sticky). Without `g`/`y`,
    /// JS starts at 0 whatever `from` is; so does this.
    pub fn exec_from(&self, s: &[u16], from: usize) -> Option<Match> {
        let mut i = if self.flags.global || self.flags.sticky {
            from
        } else {
            0
        };
        while i <= s.len() {
            if let Some(m) = self.match_at(s, i) {
                return Some(m);
            }
            if self.flags.sticky {
                return None;
            }
            // AdvanceStringIndex: by a code point with `u`.
            i += if self.flags.unicode
                && i + 1 < s.len()
                && (0xd800..0xdc00).contains(&s[i])
                && (0xdc00..0xe000).contains(&s[i + 1])
            {
                2
            } else {
                1
            };
        }
        None
    }

    /// `regexp.exec(s)` (with `lastIndex` 0).
    pub fn exec(&self, s: &[u16]) -> Option<Match> {
        self.exec_from(s, 0)
    }

    /// `regexp.test(s)` (with `lastIndex` 0).
    pub fn test(&self, s: &[u16]) -> bool {
        self.exec(s).is_some()
    }

    /// `regexp.test(s)` on a Rust string.
    pub fn test_str(&self, s: &str) -> bool {
        self.test(&s.encode_utf16().collect::<Vec<_>>())
    }
}

fn utf16_starts(s: &[u16]) -> impl Iterator<Item = usize> + '_ {
    let mut i = 0;
    std::iter::from_fn(move || {
        if i >= s.len() {
            return None;
        }
        let st = i;
        i += if (0xd800..0xdc00).contains(&s[i])
            && i + 1 < s.len()
            && (0xdc00..0xe000).contains(&s[i + 1])
        {
            2
        } else {
            1
        };
        Some(st)
    })
}

// --------------------------------------------------------------------------------
// Matcher

type Caps = Vec<Option<(usize, usize)>>;
type Cont<'k> = dyn FnMut(usize, &mut Caps) -> bool + 'k;

struct Matcher<'a> {
    re: &'a RegExp,
    s: &'a [u16],
}

impl Matcher<'_> {
    /// The character at `pos`: a code point with `u`, else a code unit.
    fn char_at(&self, pos: usize) -> Option<(u32, usize)> {
        let c = *self.s.get(pos)?;
        if self.re.flags.unicode
            && (0xd800..0xdc00).contains(&c)
            && let Some(&lo) = self.s.get(pos + 1)
            && (0xdc00..0xe000).contains(&lo)
        {
            return Some((
                0x10000 + (((c as u32) - 0xd800) << 10) + (lo as u32 - 0xdc00),
                2,
            ));
        }
        Some((c as u32, 1))
    }

    fn class_has(&self, items: &[ClassItem], c: u32) -> bool {
        items.iter().any(|it| match *it {
            ClassItem::Range(a, b) => (a..=b).contains(&c),
            ClassItem::Digit(n) => is_digit(c) != n,
            ClassItem::Word(n) => is_word_cp(c) != n,
            ClassItem::Space(n) => is_space_cp(c) != n,
        })
    }

    fn class_matches(&self, items: &[ClassItem], c: u32) -> bool {
        if !self.re.flags.ignore_case {
            return self.class_has(items, c);
        }
        let u = self.re.flags.unicode;
        let cc = canonicalize(c, u);
        [c, cc, lower_one(c), lower_one(cc)]
            .into_iter()
            .any(|x| canonicalize(x, u) == cc && self.class_has(items, x))
    }

    /// Matches one character with a single-character node; the position after it.
    fn step(&self, node: &Node, pos: usize) -> Option<usize> {
        let (c, l) = self.char_at(pos)?;
        let ok = match node {
            Node::Char(x) => {
                if self.re.flags.ignore_case {
                    canonicalize(c, self.re.flags.unicode) == *x
                } else {
                    c == *x
                }
            }
            Node::Any => self.re.flags.dot_all || !is_line_terminator(c),
            Node::Class { items, negate } => self.class_matches(items, c) != *negate,
            _ => unreachable!(),
        };
        ok.then_some(pos + l)
    }

    fn is_word_at(&self, pos: usize) -> bool {
        self.s.get(pos).is_some_and(|&c| is_js_word(c))
    }

    fn m(&self, node: &Node, pos: usize, caps: &mut Caps, k: &mut Cont<'_>) -> bool {
        match node {
            Node::Empty => k(pos, caps),
            Node::Char(_) | Node::Any | Node::Class { .. } => match self.step(node, pos) {
                Some(p) => k(p, caps),
                None => false,
            },
            Node::Start => {
                let ok = pos == 0
                    || (self.re.flags.multiline && is_line_terminator(self.s[pos - 1] as u32));
                ok && k(pos, caps)
            }
            Node::End => {
                let ok = pos == self.s.len()
                    || (self.re.flags.multiline && is_line_terminator(self.s[pos] as u32));
                ok && k(pos, caps)
            }
            Node::WordBoundary(neg) => {
                let a = pos > 0 && self.is_word_at(pos - 1);
                let b = self.is_word_at(pos);
                ((a != b) != *neg) && k(pos, caps)
            }
            Node::Group(inner, None) => self.m(inner, pos, caps, k),
            Node::Group(inner, Some(i)) => {
                let i = *i;
                self.m(inner, pos, caps, &mut |p, caps: &mut Caps| {
                    let old = caps[i];
                    caps[i] = Some((pos, p));
                    if k(p, caps) {
                        true
                    } else {
                        caps[i] = old;
                        false
                    }
                })
            }
            Node::Look {
                node,
                behind,
                negate,
            } => {
                let saved = caps.clone();
                let found = if *behind {
                    (0..=pos)
                        .rev()
                        .any(|st| self.m(node, st, caps, &mut |p, _| p == pos))
                } else {
                    self.m(node, pos, caps, &mut |_, _| true)
                };
                if *negate {
                    *caps = saved;
                    !found && k(pos, caps)
                } else if found {
                    if k(pos, caps) {
                        true
                    } else {
                        *caps = saved;
                        false
                    }
                } else {
                    *caps = saved;
                    false
                }
            }
            Node::BackRef(i) => {
                let Some((a, b)) = caps.get(*i).copied().flatten() else {
                    return k(pos, caps);
                };
                let len = b - a;
                if pos + len > self.s.len() {
                    return false;
                }
                let ic = self.re.flags.ignore_case;
                let u = self.re.flags.unicode;
                let same = (0..len).all(|j| {
                    let (x, y) = (self.s[a + j] as u32, self.s[pos + j] as u32);
                    x == y || (ic && canonicalize(x, u) == canonicalize(y, u))
                });
                same && k(pos + len, caps)
            }
            Node::Seq(nodes) => self.seq(nodes, pos, caps, k),
            Node::Alt(alts) => alts.iter().any(|a| self.m(a, pos, caps, k)),
            Node::Repeat(r) => self.rep(r, r.min, r.max, pos, caps, k),
        }
    }

    fn seq(&self, nodes: &[Node], pos: usize, caps: &mut Caps, k: &mut Cont<'_>) -> bool {
        match nodes.split_first() {
            None => k(pos, caps),
            Some((first, rest)) => self.m(first, pos, caps, &mut |p, caps: &mut Caps| {
                self.seq(rest, p, caps, k)
            }),
        }
    }

    /// `RepeatMatcher(m, min, max, greedy, x, c, parenIndex, parenCount)`.
    fn rep(
        &self,
        r: &Repeat,
        min: u32,
        max: Option<u32>,
        pos: usize,
        caps: &mut Caps,
        k: &mut Cont<'_>,
    ) -> bool {
        if matches!(r.node, Node::Char(_) | Node::Any | Node::Class { .. }) {
            return self.rep_simple(r, min, max, pos, caps, k);
        }
        if max == Some(0) {
            return k(pos, caps);
        }
        if min > 0 {
            return self.rep_once(r, min, max, pos, caps, k);
        }
        // Greedy: one more iteration first; lazy: the continuation first.
        if !r.greedy && k(pos, caps) {
            return true;
        }
        if self.rep_once(r, min, max, pos, caps, k) {
            return true;
        }
        r.greedy && k(pos, caps)
    }

    fn rep_once(
        &self,
        r: &Repeat,
        min: u32,
        max: Option<u32>,
        pos: usize,
        caps: &mut Caps,
        k: &mut Cont<'_>,
    ) -> bool {
        let range = r.first_group..r.first_group + r.group_count;
        let saved: Vec<_> = caps[range.clone()].to_vec();
        for c in &mut caps[range.clone()] {
            *c = None;
        }
        let ok = self.m(&r.node, pos, caps, &mut |p, caps: &mut Caps| {
            if min == 0 && p == pos {
                return false;
            }
            self.rep(
                r,
                min.saturating_sub(1),
                max.map(|m| m - 1),
                p,
                caps,
                &mut *k,
            )
        });
        if !ok {
            caps[range].copy_from_slice(&saved);
        }
        ok
    }

    /// A quantified single character: no captures and no empty iterations, so the
    /// candidate ends are computed in one pass.
    fn rep_simple(
        &self,
        r: &Repeat,
        min: u32,
        max: Option<u32>,
        pos: usize,
        caps: &mut Caps,
        k: &mut Cont<'_>,
    ) -> bool {
        let mut ends = vec![pos];
        let mut p = pos;
        while max.is_none_or(|m| ((ends.len() - 1) as u32) < m) {
            match self.step(&r.node, p) {
                Some(np) => {
                    p = np;
                    ends.push(p);
                }
                None => break,
            }
        }
        let n = ends.len() - 1;
        let min = min as usize;
        if n < min {
            return false;
        }
        if r.greedy {
            (min..=n).rev().any(|i| k(ends[i], caps))
        } else {
            (min..=n).any(|i| k(ends[i], caps))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }
    fn ex(re: &str, flags: &str, s: &str) -> Option<Vec<Option<String>>> {
        let r = RegExp::new(re, flags).unwrap();
        let s = u(s);
        r.exec(&s).map(|m| {
            (0..m.count())
                .map(|i| m.text(&s, i).map(String::from_utf16_lossy))
                .collect()
        })
    }
    fn g(v: &[Option<&str>]) -> Option<Vec<Option<String>>> {
        Some(v.iter().map(|x| x.map(str::to_string)).collect())
    }

    #[test]
    fn basics() {
        assert_eq!(ex("a+b", "", "xaaab"), g(&[Some("aaab")]));
        assert_eq!(ex("^(?:a|ab)c", "", "abc"), g(&[Some("abc")]));
        assert_eq!(ex("(a)|(b)", "", "b"), g(&[Some("b"), None, Some("b")]));
        assert_eq!(ex("a*?b", "", "aab"), g(&[Some("aab")]));
        assert_eq!(ex("<.*?>", "", "<a><b>"), g(&[Some("<a>")]));
        assert_eq!(ex("x{2,3}", "", "xxxx"), g(&[Some("xxx")]));
        assert_eq!(ex("x{2,}?", "", "xxxx"), g(&[Some("xx")]));
        assert_eq!(ex("a{,2}", "", "a{,2}"), g(&[Some("a{,2}")]));
        assert_eq!(ex("\\bfoo\\b", "", "a foo."), g(&[Some("foo")]));
        assert_eq!(ex("foo(?=bar)", "", "foobaz foobar"), g(&[Some("foo")]));
        assert_eq!(ex("foo(?!bar)", "", "foobar foobaz"), g(&[Some("foo")]));
        assert_eq!(ex("(?<=\\$)\\d+", "", "a1 $42"), g(&[Some("42")]));
        assert_eq!(ex("(?<!\\$)\\b\\d+", "", "$4 5"), g(&[Some("5")]));
        assert_eq!(ex("(a)\\1", "", "xaa"), g(&[Some("aa"), Some("a")]));
        assert_eq!(
            ex("(?<q>['\"]).*?\\k<q>", "", "x'a\"b'"),
            g(&[Some("'a\"b'"), Some("'")])
        );
        assert_eq!(ex("[^\\s\\]]+", "", "  ab]"), g(&[Some("ab")]));
        assert_eq!(ex("[\\w-]+", "", "a-b c"), g(&[Some("a-b")]));
        assert_eq!(ex("\\x41\\u0042\\103", "", "ABC"), g(&[Some("ABC")]));
        assert_eq!(ex("^$", "m", "a\n\nb"), g(&[Some("")]));
        assert_eq!(ex("a.c", "", "a\nc abc"), g(&[Some("abc")]));
        assert_eq!(ex("a.c", "s", "a\nc"), g(&[Some("a\nc")]));
        // JS clears captures of each iteration.
        assert_eq!(ex("(?:(a)|b)+", "", "ab"), g(&[Some("ab"), None]));
        // Empty iterations stop a `*`.
        assert_eq!(ex("(a*)*b", "", "b"), g(&[Some("b"), None]));
        assert_eq!(ex("(a|)*", "", "aa"), g(&[Some("aa"), Some("a")]));
    }

    #[test]
    fn ignore_case() {
        assert_eq!(ex("ABC", "i", "xabc"), g(&[Some("abc")]));
        assert_eq!(ex("[a-z]+", "i", "XyZ1"), g(&[Some("XyZ")]));
        assert_eq!(ex("[^a-z]", "i", "Q1"), g(&[Some("1")]));
        assert_eq!(ex("é", "i", "É"), g(&[Some("É")]));
        // Non-ASCII never canonicalizes to ASCII without `u`: ſ (U+017F) is not s.
        assert_eq!(ex("s", "i", "\u{17f}"), None);
        assert_eq!(ex("(a)\\1", "i", "aA"), g(&[Some("aA"), Some("a")]));
        // With `u`, K (U+212A, Kelvin) folds to k.
        assert_eq!(ex("\\u212a", "iu", "k"), g(&[Some("k")]));
        assert_eq!(ex("K", "iu", "\u{212a}"), g(&[Some("\u{212a}")]));
    }

    #[test]
    fn utf16() {
        // Without `u` an astral character is two units: `.` takes one.
        let s = u("\u{1F600}x");
        let r = RegExp::new("^.", "").unwrap();
        assert_eq!(r.exec(&s).unwrap().len(), 1);
        let r = RegExp::new("^.", "u").unwrap();
        assert_eq!(r.exec(&s).unwrap().len(), 2);
        let r = RegExp::new("\\u{1F600}", "u").unwrap();
        assert_eq!(r.exec(&s).unwrap().len(), 2);
        let r = RegExp::new("[\\uD83D]", "").unwrap();
        assert_eq!(r.exec(&s).unwrap().index(), 0);
        let r = RegExp::new("x", "g").unwrap();
        assert_eq!(r.exec_from(&s, 1).unwrap().index(), 2);
    }

    #[test]
    fn annex_b_and_errors() {
        assert_eq!(ex("\\8", "", "8"), g(&[Some("8")]));
        assert_eq!(ex("a]", "", "a]"), g(&[Some("a]")]));
        assert_eq!(ex("\\c1", "", "\\c1"), g(&[Some("\\c1")]));
        assert_eq!(ex("(?=a)*a", "", "a"), g(&[Some("a")]));
        for (re, fl) in [
            ("a**", ""),
            ("(", ""),
            ("a)", ""),
            ("[b-a]", ""),
            ("x{3,2}", ""),
            ("\\k<n>", "u"),
            ("a{", "u"),
            ("(?<=a)*", ""),
            ("a", "gg"),
            ("\\p", "u"),
        ] {
            assert!(RegExp::new(re, fl).is_err(), "{re} /{fl}");
        }
    }

    #[test]
    fn long_simple_repeat_does_not_recurse() {
        let s = vec![b'a' as u16; 200_000];
        let r = RegExp::new("^a*$", "").unwrap();
        assert_eq!(r.exec(&s).unwrap().len(), 200_000);
        let r = RegExp::new("^[^\"]*?$", "").unwrap();
        assert_eq!(r.exec(&s).unwrap().len(), 200_000);
    }
}
