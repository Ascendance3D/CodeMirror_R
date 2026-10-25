//! `src/util/bidi.js`: the simplified Unicode bidi algorithm CM5 uses for line
//! ordering, plus its two helpers. `getOrder` caches on the line object in JS; here
//! it is a pure function ([`bidi_ordering`]).

use crate::text::is_js_space;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BidiSpan {
    pub level: u8,
    pub from: usize,
    pub to: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Ltr,
    Rtl,
}

/// `iterateBidiSections(order, from, to, f)`. `f` receives `(from, to, dir, index)`;
/// `index` is `None` where the JS passes `undefined`.
pub fn iterate_bidi_sections<F: FnMut(usize, usize, Direction, Option<usize>)>(
    order: Option<&[BidiSpan]>,
    from: usize,
    to: usize,
    mut f: F,
) {
    let Some(order) = order else {
        return f(from, to, Direction::Ltr, Some(0));
    };
    let mut found = false;
    for (i, part) in order.iter().enumerate() {
        if part.from < to && part.to > from || from == to && part.to == from {
            f(
                part.from.max(from),
                part.to.min(to),
                if part.level == 1 {
                    Direction::Rtl
                } else {
                    Direction::Ltr
                },
                Some(i),
            );
            found = true;
        }
    }
    if !found {
        f(from, to, Direction::Ltr, None);
    }
}

/// `getBidiPartAt(order, ch, sticky)`. Returns `(result, bidiOther)`; the JS keeps the
/// second value in the module-level `bidiOther` variable.
pub fn get_bidi_part_at(
    order: &[BidiSpan],
    ch: usize,
    sticky_before: bool,
) -> (Option<usize>, Option<usize>) {
    let mut found = None;
    let mut bidi_other = None;
    for (i, cur) in order.iter().enumerate() {
        if cur.from < ch && cur.to > ch {
            return (Some(i), bidi_other);
        }
        if cur.to == ch {
            if cur.from != cur.to && sticky_before {
                found = Some(i);
            } else {
                bidi_other = Some(i);
            }
        }
        if cur.from == ch {
            if cur.from != cur.to && !sticky_before {
                found = Some(i);
            } else {
                bidi_other = Some(i);
            }
        }
    }
    (if found.is_some() { found } else { bidi_other }, bidi_other)
}

const LOW_TYPES: &[u8] = b"bbbbbbbbbtstwsbbbbbbbbbbbbbbssstwNN%%%NNNNNN,N,N1111111111NNNNNNNLLLLLLLLLLLLLLLLLLLLLLLLLLNNNNNNLLLLLLLLLLLLLLLLLLLLLLLLLLNNNNbbbbbbsbbbbbbbbbbbbbbbbbbbbbbbbbb,N%%%%NNNNLNNNNN%%11NLNNN1LNNNNNLLLLLLLLLLLLLLLLLLLLLLLNLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLN";
const ARABIC_TYPES: &[u8] = b"nnnnnnNNr%%r,rNNmmmmmmmmmmmrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrmmmmmmmmmmmmmmmmmmmmmnnnnnnnnnn%nnrrrmrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrmmmmmmmnNmmmmmmrrmmNmmmmrr1111111111";

fn char_type(code: u16) -> u8 {
    // `lowTypes.charAt(code)` / `arabicTypes.charAt(..)` return "" past the end of the
    // tables; "" never equals any type letter, so model it as 0.
    let at = |t: &[u8], i: usize| t.get(i).copied().unwrap_or(0);
    if code <= 0xf7 {
        at(LOW_TYPES, code as usize)
    } else if (0x590..=0x5f4).contains(&code) {
        b'R'
    } else if (0x600..=0x6f9).contains(&code) {
        at(ARABIC_TYPES, (code - 0x600) as usize)
    } else if (0x6ee..=0x8ac).contains(&code) {
        b'r'
    } else if (0x2000..=0x200b).contains(&code) {
        b'w'
    } else if code == 0x200c {
        b'b'
    } else {
        b'L'
    }
}

fn is_neutral(t: u8) -> bool {
    matches!(t, b's' | b't' | b'w' | b'N')
}
fn is_strong(t: u8) -> bool {
    matches!(t, b'L' | b'R' | b'r')
}
fn counts_as_left(t: u8) -> bool {
    matches!(t, b'L' | b'b' | b'1' | b'n')
}
fn counts_as_num(t: u8) -> bool {
    matches!(t, b'1' | b'n')
}

/// `bidiOrdering(str, direction)`: `None` (JS `false`) when the text is plain
/// left-to-right, otherwise the spans in visual order.
pub fn bidi_ordering(s: &[u16], direction: Direction) -> Option<Vec<BidiSpan>> {
    let outer_type = if direction == Direction::Ltr {
        b'L'
    } else {
        b'R'
    };
    let bidi_re = |c: u16| {
        (0x0590..=0x05f4).contains(&c)
            || (0x0600..=0x06ff).contains(&c)
            || (0x0700..=0x08ac).contains(&c)
    };
    if s.is_empty() || direction == Direction::Ltr && !s.iter().any(|&c| bidi_re(c)) {
        return None;
    }
    let len = s.len();
    let mut types: Vec<u8> = s.iter().map(|&c| char_type(c)).collect();

    // W1.
    let mut prev = outer_type;
    for t in types.iter_mut() {
        if *t == b'm' { *t = prev } else { prev = *t }
    }
    // W2 + W3.
    let mut cur = outer_type;
    for t in types.iter_mut() {
        if *t == b'1' && cur == b'r' {
            *t = b'n';
        } else if is_strong(*t) {
            cur = *t;
            if *t == b'r' {
                *t = b'R';
            }
        }
    }
    // W4. Note `prev = type` keeps the *original* type of the previous char.
    if len > 0 {
        let mut prev = types[0];
        let mut i = 1;
        while i + 1 < len {
            let t = types[i];
            if t == b'+' && prev == b'1' && types[i + 1] == b'1' {
                types[i] = b'1';
            } else if t == b',' && prev == types[i + 1] && (prev == b'1' || prev == b'n') {
                types[i] = prev;
            }
            prev = t;
            i += 1;
        }
    }
    // W5 + W6. (The JS checks for a previous "!" type, which never occurs.)
    let mut i = 0;
    while i < len {
        let t = types[i];
        if t == b',' {
            types[i] = b'N';
        } else if t == b'%' {
            let mut end = i + 1;
            while end < len && types[end] == b'%' {
                end += 1;
            }
            let replace = if (i > 0 && types[i - 1] == b'!') || (end < len && types[end] == b'1') {
                b'1'
            } else {
                b'N'
            };
            for t in &mut types[i..end] {
                *t = replace;
            }
            i = end - 1;
        }
        i += 1;
    }
    // W7.
    let mut cur = outer_type;
    for t in types.iter_mut() {
        if cur == b'L' && *t == b'1' {
            *t = b'L';
        } else if is_strong(*t) {
            cur = *t;
        }
    }
    // N1 + N2.
    let mut i = 0;
    while i < len {
        if is_neutral(types[i]) {
            let mut end = i + 1;
            while end < len && is_neutral(types[end]) {
                end += 1;
            }
            let before = (if i > 0 { types[i - 1] } else { outer_type }) == b'L';
            let after = (if end < len { types[end] } else { outer_type }) == b'L';
            let replace = if before == after {
                if before { b'L' } else { b'R' }
            } else {
                outer_type
            };
            for t in &mut types[i..end] {
                *t = replace;
            }
            i = end - 1;
        }
        i += 1;
    }

    let mut order: Vec<BidiSpan> = Vec::new();
    let mut i = 0;
    while i < len {
        if counts_as_left(types[i]) {
            let start = i;
            i += 1;
            while i < len && counts_as_left(types[i]) {
                i += 1;
            }
            order.push(BidiSpan {
                level: 0,
                from: start,
                to: i,
            });
        } else {
            let mut pos = i;
            let mut at = order.len();
            let is_rtl = if direction == Direction::Rtl { 1 } else { 0 };
            i += 1;
            while i < len && types[i] != b'L' {
                i += 1;
            }
            let mut j = pos;
            while j < i {
                if counts_as_num(types[j]) {
                    if pos < j {
                        order.insert(
                            at,
                            BidiSpan {
                                level: 1,
                                from: pos,
                                to: j,
                            },
                        );
                        at += is_rtl;
                    }
                    let nstart = j;
                    j += 1;
                    while j < i && counts_as_num(types[j]) {
                        j += 1;
                    }
                    order.insert(
                        at,
                        BidiSpan {
                            level: 2,
                            from: nstart,
                            to: j,
                        },
                    );
                    at += is_rtl;
                    pos = j;
                } else {
                    j += 1;
                }
            }
            if pos < i {
                order.insert(
                    at,
                    BidiSpan {
                        level: 1,
                        from: pos,
                        to: i,
                    },
                );
            }
        }
    }
    if direction == Direction::Ltr {
        let lead = s.iter().take_while(|&&c| is_js_space(c)).count();
        if order[0].level == 1 && lead > 0 {
            order[0].from = lead;
            order.insert(
                0,
                BidiSpan {
                    level: 0,
                    from: 0,
                    to: lead,
                },
            );
        }
        let trail = s.iter().rev().take_while(|&&c| is_js_space(c)).count();
        let last = order.len() - 1;
        if order[last].level == 1 && trail > 0 {
            order[last].to -= trail;
            order.push(BidiSpan {
                level: 0,
                from: len - trail,
                to: len,
            });
        }
    }
    if direction == Direction::Rtl {
        order.reverse();
    }
    Some(order)
}
