//! Pure helpers from `src/util/misc.js`. DOM/timer helpers (`Delayed`, `bind`,
//! `createObj`, `copyObj`, `Pass`, `scrollerGap`, selection option objects) are not
//! ported: they are either JS object plumbing or view-only.

use super::extending_chars::EXTENDING_CHARS;
use crate::text::{index_of, is_js_space, is_js_word};

/// `countColumn(string, end, tabSize, startIndex, startValue)`.
///
/// `end == None` means "up to the first non-whitespace character"
/// (`string.search(/[^\s ]/)`).
pub fn count_column(
    s: &[u16],
    end: Option<usize>,
    tab_size: i64,
    start_index: usize,
    start_value: i64,
) -> i64 {
    let end = match end {
        Some(e) => e as i64,
        None => s.iter().position(|&c| !is_js_space(c)).unwrap_or(s.len()) as i64,
    };
    let mut i = start_index;
    let mut n = start_value;
    // The JS `indexOf("\t", i)` scans to the end of the line; a tab at or past `end`
    // takes the same branch as no tab, so the scan stops at `end`. Same results, but
    // linear instead of quadratic when modes ask `column()` all along one long line
    // (C4: javascript `pushlex` on single-line JSON files).
    let limit = (end.max(0) as usize).min(s.len());
    loop {
        let next_tab = if i < limit {
            s[i..limit]
                .iter()
                .position(|&c| c == 0x09)
                .map(|p| (p + i) as i64)
                .unwrap_or(-1)
        } else {
            -1
        };
        if next_tab < 0 || next_tab >= end {
            return n + (end - i as i64);
        }
        n += next_tab - i as i64;
        n += tab_size - (n % tab_size);
        i = next_tab as usize + 1;
    }
}

/// `findColumn(string, goal, tabSize)`: the inverse of `countColumn`.
pub fn find_column(s: &[u16], goal: i64, tab_size: i64) -> i64 {
    let mut pos: i64 = 0;
    let mut col: i64 = 0;
    loop {
        let next_tab = index_of(s, &[0x09], pos as usize)
            .map(|x| x as i64)
            .unwrap_or(s.len() as i64);
        let skipped = next_tab - pos;
        if next_tab == s.len() as i64 || col + skipped >= goal {
            return pos + skipped.min(goal - col);
        }
        col += next_tab - pos;
        col += tab_size - (col % tab_size);
        pos = next_tab + 1;
        if col >= goal {
            return pos;
        }
    }
}

/// `spaceStr(n)`.
pub fn space_str(n: usize) -> String {
    " ".repeat(n)
}

/// `insertSorted(array, value, score)`: inserts after every element whose score is
/// `<=` the new one.
pub fn insert_sorted<T, F: Fn(&T) -> f64>(array: &mut Vec<T>, value: T, score: F) {
    let priority = score(&value);
    let mut pos = 0;
    while pos < array.len() && score(&array[pos]) <= priority {
        pos += 1;
    }
    array.insert(pos, value);
}

fn non_ascii_single_case_word_char(c: u16) -> bool {
    matches!(c, 0x00df | 0x0587 | 0x0590..=0x05f4 | 0x0600..=0x06ff | 0x3040..=0x309f | 0x30a0..=0x30ff
        | 0x3400..=0x4db5 | 0x4e00..=0x9fcc | 0xac00..=0xd7af)
}

/// `ch.toUpperCase() != ch.toLowerCase()` on a UTF-16 string. Lone surrogates map to
/// themselves in JS, so only decodable scalars can differ.
fn has_case(s: &[u16]) -> bool {
    char::decode_utf16(s.iter().copied()).any(|r| match r {
        Ok(c) => !c.to_uppercase().eq(c.to_lowercase()),
        Err(_) => false,
    })
}

/// `isWordCharBasic(ch)`. Works on a whole string like the JS (`/\w/.test` matches
/// anywhere; `ch > "\x80"` is a string comparison).
pub fn is_word_char_basic(ch: &[u16]) -> bool {
    if ch.iter().any(|&c| is_js_word(c)) {
        return true;
    }
    let gt_x80 = match ch.first() {
        Some(&c) => c > 0x80 || (c == 0x80 && ch.len() > 1),
        None => false,
    };
    gt_x80 && (has_case(ch) || ch.iter().any(|&c| non_ascii_single_case_word_char(c)))
}

/// A `helper` regexp for [`is_word_char`]: the JS checks whether its source contains
/// `\w`, then calls `helper.test(ch)`.
pub struct WordCharHelper<'a> {
    pub source_contains_w: bool,
    pub test: &'a dyn Fn(&[u16]) -> bool,
}

/// `isWordChar(ch, helper)`.
pub fn is_word_char(ch: &[u16], helper: Option<&WordCharHelper>) -> bool {
    match helper {
        None => is_word_char_basic(ch),
        Some(h) => {
            if h.source_contains_w && is_word_char_basic(ch) {
                return true;
            }
            (h.test)(ch)
        }
    }
}

/// `isEmpty(obj)` has no Rust equivalent worth porting (JS object plumbing).
///
/// `isExtendingChar(ch)` for a single UTF-16 unit.
pub fn is_extending_char(c: u16) -> bool {
    c >= 768
        && EXTENDING_CHARS
            .binary_search_by(|&(lo, hi)| {
                if hi < c {
                    std::cmp::Ordering::Less
                } else if lo > c {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
}

/// `skipExtendingChars(str, pos, dir)`.
pub fn skip_extending_chars(s: &[u16], mut pos: i64, dir: i64) -> i64 {
    // `str.charAt(pos)` is "" outside the string, and "" is not extending.
    let at = |p: i64| {
        if p >= 0 && (p as usize) < s.len() {
            is_extending_char(s[p as usize])
        } else {
            false
        }
    };
    while (if dir < 0 {
        pos > 0
    } else {
        pos < s.len() as i64
    }) && at(pos)
    {
        pos += dir;
    }
    pos
}

/// `findFirst(pred, from, to)`: the value in `[from; to]` closest to `from` that
/// satisfies `pred`, assuming `to` does. `from` may be greater than `to`.
pub fn find_first<F: FnMut(i64) -> bool>(mut pred: F, mut from: i64, mut to: i64) -> i64 {
    let dir = if from > to { -1 } else { 1 };
    loop {
        if from == to {
            return from;
        }
        let mid_f = (from + to) as f64 / 2.0;
        let mid = if dir < 0 { mid_f.ceil() } else { mid_f.floor() } as i64;
        if mid == from {
            return if pred(mid) { from } else { to };
        }
        if pred(mid) {
            to = mid;
        } else {
            from = mid + dir;
        }
    }
}
