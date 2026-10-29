//! `src/input/movement.js` plus `findPosH` / `findPosV` from `src/edit/methods.js`
//! and the line-start helpers of `src/edit/commands.js`.

use cm_core::Pos;
use cm_core::pos::{Sticky, equal_cursor_pos};
use cm_core::text::{Text, is_js_space};
use cm_core::util::bidi::{BidiSpan, Direction, bidi_ordering, get_bidi_part_at};
use cm_core::util::misc::{find_first, is_word_char, skip_extending_chars};

use crate::editor::{CoordSys, Editor};
use crate::measure::{PosInfo, Rect};

/// Horizontal motion units (`findPosH`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitH {
    Codepoint,
    Char,
    Column,
    Word,
    Group,
}

impl UnitH {
    pub fn parse(s: &str) -> Option<UnitH> {
        Some(match s {
            "codepoint" => UnitH::Codepoint,
            "char" => UnitH::Char,
            "column" => UnitH::Column,
            "word" => UnitH::Word,
            "group" => UnitH::Group,
            _ => return None,
        })
    }
}

/// Vertical motion units (`findPosV`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitV {
    Line,
    Page,
}

/// `getOrder(line, direction)`.
pub fn get_order(text: &[u16], dir: Direction) -> Option<Vec<BidiSpan>> {
    bidi_ordering(text, dir)
}

fn sticky_of(dir: i64) -> Option<Sticky> {
    Some(if dir < 0 {
        Sticky::After
    } else {
        Sticky::Before
    })
}

/// `moveCharLogically(line, ch, dir)`.
pub fn move_char_logically(text: &[u16], ch: i64, dir: i64) -> Option<i64> {
    let target = skip_extending_chars(text, ch + dir, dir);
    if target < 0 || target > text.len() as i64 {
        None
    } else {
        Some(target)
    }
}

/// `moveLogically(line, start, dir)`.
pub fn move_logically(text: &[u16], start: Pos, dir: i64) -> Option<Pos> {
    move_char_logically(text, start.ch, dir)
        .map(|ch| Pos::with_sticky(start.line, ch, sticky_of(dir)))
}

/// `endOfLine(visually, cm, lineObj, lineNo, dir)`.
pub fn end_of_line(cm: &Editor, visually: bool, text: &[u16], line_no: i64, dir: i64) -> Pos {
    let rtl_doc = cm.doc.direction() == Direction::Rtl;
    if visually {
        let dir = if rtl_doc { -dir } else { dir };
        if let Some(order) = get_order(text, cm.doc.direction()) {
            let part = if dir < 0 {
                *order.last().unwrap()
            } else {
                order[0]
            };
            let move_in_storage_order = (dir < 0) == (part.level == 1);
            let sticky = if move_in_storage_order {
                Sticky::After
            } else {
                Sticky::Before
            };
            let ch;
            if part.level > 0 || rtl_doc {
                let layout = cm.layout_for(line_no, text);
                let mut c = if dir < 0 { text.len() as i64 - 1 } else { 0 };
                let target_top = layout.measure_char(text, c).top;
                c = find_first(
                    |c| layout.measure_char(text, c).top == target_top,
                    if (dir < 0) == (part.level == 1) {
                        part.from as i64
                    } else {
                        part.to as i64 - 1
                    },
                    c,
                );
                if sticky == Sticky::Before {
                    c = move_char_logically(text, c, 1).unwrap_or(c);
                }
                ch = c;
            } else {
                ch = if dir < 0 {
                    part.to as i64
                } else {
                    part.from as i64
                };
            }
            return Pos::with_sticky(line_no, ch, Some(sticky));
        }
    }
    Pos::with_sticky(
        line_no,
        if dir < 0 { text.len() as i64 } else { 0 },
        Some(if dir < 0 {
            Sticky::Before
        } else {
            Sticky::After
        }),
    )
}

/// `moveVisually(cm, line, start, dir)`.
pub fn move_visually(cm: &Editor, text: &[u16], start: Pos, dir: i64) -> Option<Pos> {
    let Some(bidi) = get_order(text, cm.doc.direction()) else {
        return move_logically(text, start, dir);
    };
    let mut start = start;
    let len = text.len() as i64;
    if start.ch >= len {
        start.ch = len;
        start.sticky = Some(Sticky::Before);
    } else if start.ch <= 0 {
        start.ch = 0;
        start.sticky = Some(Sticky::After);
    }
    let (part_pos, _) = get_bidi_part_at(
        &bidi,
        start.ch as usize,
        start.sticky == Some(Sticky::Before),
    );
    let part_pos = part_pos.unwrap_or(0);
    let part = bidi[part_pos];
    let ltr_doc = cm.doc.direction() == Direction::Ltr;
    if ltr_doc
        && part.level % 2 == 0
        && (if dir > 0 {
            part.to as i64 > start.ch
        } else {
            (part.from as i64) < start.ch
        })
    {
        // Case 1: we move within an ltr part in an ltr editor.
        return move_logically(text, start, dir);
    }
    let mv = |pos: i64, dir: i64| move_char_logically(text, pos, dir);
    let layout = if cm.options.line_wrapping {
        Some(cm.layout_for(start.line, text))
    } else {
        None
    };
    let get_extent = |ch: i64| -> (i64, i64) {
        match &layout {
            None => (0, len),
            Some(l) => l.wrapped_line_extent_char(text, ch),
        }
    };
    let ext_from = if start.sticky == Some(Sticky::Before) {
        mv(start.ch, -1).unwrap_or(-1)
    } else {
        start.ch
    };
    let wrapped = get_extent(ext_from);

    if !ltr_doc || part.level == 1 {
        let mis = (part.level == 1) == (dir < 0);
        let ch = mv(start.ch, if mis { 1 } else { -1 });
        if let Some(ch) = ch {
            let ok = if !mis {
                ch >= part.from as i64 && ch >= wrapped.0
            } else {
                ch <= part.to as i64 && ch <= wrapped.1
            };
            if ok {
                // Case 2: within an rtl part or in an rtl editor on the same visual line.
                return Some(Pos::with_sticky(
                    start.line,
                    ch,
                    Some(if mis { Sticky::Before } else { Sticky::After }),
                ));
            }
        }
    }

    // Case 3: leave the current bidi part.
    let search = |mut part_pos: i64, dir: i64, ext: (i64, i64)| -> Option<Pos> {
        let get_res = |ch: i64, mis: bool| {
            if mis {
                Pos::with_sticky(start.line, mv(ch, 1).unwrap_or(-1), Some(Sticky::Before))
            } else {
                Pos::with_sticky(start.line, ch, Some(Sticky::After))
            }
        };
        while part_pos >= 0 && (part_pos as usize) < bidi.len() {
            let part = bidi[part_pos as usize];
            let mis = (dir > 0) == (part.level != 1);
            let mut ch = if mis {
                ext.0
            } else {
                mv(ext.1, -1).unwrap_or(-1)
            };
            if (part.from as i64) <= ch && ch < part.to as i64 {
                return Some(get_res(ch, mis));
            }
            ch = if mis {
                part.from as i64
            } else {
                mv(part.to as i64, -1).unwrap_or(-1)
            };
            if ext.0 <= ch && ch < ext.1 {
                return Some(get_res(ch, mis));
            }
            part_pos += dir;
        }
        None
    };
    // Case 3a: other bidi parts on the same visual line.
    if let Some(r) = search(part_pos as i64 + dir, dir, wrapped) {
        return Some(r);
    }
    // Case 3b: other bidi parts on the next visual line.
    let next_ch = if dir > 0 {
        Some(wrapped.1)
    } else {
        mv(wrapped.0, -1)
    };
    if let Some(nc) = next_ch {
        if !(dir > 0 && nc == len) {
            if let Some(r) = search(
                if dir > 0 { 0 } else { bidi.len() as i64 - 1 },
                dir,
                get_extent(nc),
            ) {
                return Some(r);
            }
        }
    }
    // Case 4: nowhere to move.
    None
}

/// `findPosH(doc, pos, dir, unit, visually)`; the bool is `hitSide`.
pub fn find_pos_h(cm: &Editor, pos: Pos, dir: i64, unit: UnitH, visually: bool) -> (Pos, bool) {
    let doc = &cm.doc;
    let old_pos = pos;
    let orig_dir = dir;
    let mut dir = dir;
    let mut pos = pos;
    let mut text: Text = doc.get_line_utf16(pos.line).unwrap_or_default();
    let line_dir = if visually && doc.direction() == Direction::Rtl {
        -dir
    } else {
        dir
    };
    let first = doc.first_line();
    let size = doc.line_count() as i64;

    // `moveOnce(boundToLine)`; needs `dir` as it is when called.
    let move_once = |pos: &mut Pos, text: &mut Text, dir: i64, bound_to_line: bool| -> bool {
        let next: Option<Pos> = match unit {
            UnitH::Codepoint => {
                let idx = pos.ch + if dir > 0 { 0 } else { -1 };
                if idx < 0 || idx >= text.len() as i64 {
                    None
                } else {
                    let ch = text[idx as usize];
                    let astral = if dir > 0 {
                        (0xd800..0xdc00).contains(&ch)
                    } else {
                        (0xdc00..0xdfff).contains(&ch)
                    };
                    let n = (pos.ch + dir * if astral { 2 } else { 1 })
                        .min(text.len() as i64)
                        .max(0);
                    // The JS stores `-dir` (a number) as the sticky value; no
                    // consumer distinguishes it from null.
                    Some(Pos::with_sticky(pos.line, n, None))
                }
            }
            _ if visually => move_visually(cm, text, *pos, dir),
            _ => move_logically(text, *pos, dir),
        };
        match next {
            None => {
                // `findNextLine()`
                let l = pos.line + line_dir;
                if !bound_to_line && l >= first && l < first + size {
                    *pos = Pos::with_sticky(l, pos.ch, pos.sticky);
                    *text = doc.get_line_utf16(l).unwrap_or_default();
                    *pos = end_of_line(cm, visually, text, pos.line, line_dir);
                    true
                } else {
                    false
                }
            }
            Some(n) => {
                *pos = n;
                true
            }
        }
    };

    match unit {
        UnitH::Char | UnitH::Codepoint => {
            move_once(&mut pos, &mut text, dir, false);
        }
        UnitH::Column => {
            move_once(&mut pos, &mut text, dir, true);
        }
        UnitH::Word | UnitH::Group => {
            let group = unit == UnitH::Group;
            let mut saw_type: Option<char> = None;
            let mut first_iter = true;
            loop {
                if dir < 0 && !move_once(&mut pos, &mut text, dir, !first_iter) {
                    break;
                }
                // `lineObj.text.charAt(pos.ch) || "\n"`
                let cur: Option<u16> = if pos.ch >= 0 && (pos.ch as usize) < text.len() {
                    Some(text[pos.ch as usize])
                } else {
                    None
                };
                let is_nl = cur.is_none();
                let mut ty: Option<char> = match cur {
                    Some(c) if is_word_char(&[c], None) => Some('w'),
                    _ if group && is_nl => Some('n'),
                    Some(c) if !group || is_js_space(c) => None,
                    None if !group => None,
                    _ => Some('p'),
                };
                if group && !first_iter && ty.is_none() {
                    ty = Some('s');
                }
                if saw_type.is_some() && saw_type != ty {
                    if dir < 0 {
                        dir = 1;
                        move_once(&mut pos, &mut text, dir, false);
                        pos.sticky = Some(Sticky::After);
                    }
                    break;
                }
                if ty.is_some() {
                    saw_type = ty;
                }
                if dir > 0 && !move_once(&mut pos, &mut text, dir, !first_iter) {
                    break;
                }
                first_iter = false;
            }
        }
    }
    let result = doc.skip_atomic(pos, Some(old_pos), orig_dir, true);
    let hit = equal_cursor_pos(&old_pos, &result);
    (result, hit)
}

/// `findPosV(cm, pos, dir, unit)`: `pos` is a "div" box.
pub fn find_pos_v(cm: &Editor, pos: Rect, dir: i64, unit: UnitV) -> (PosInfo, bool) {
    let x = pos.left;
    let mut y = match unit {
        UnitV::Page => {
            let page_size = cm.page_size();
            let move_amount = (page_size - 0.5 * cm.metrics.text_height).max(3.0);
            (if dir > 0 { pos.bottom } else { pos.top }) + dir as f64 * move_amount
        }
        UnitV::Line => {
            if dir > 0 {
                pos.bottom + 3.0
            } else {
                pos.top - 3.0
            }
        }
    };
    let doc_height = cm.doc_height();
    loop {
        let target = cm.coords_char(x, y);
        if target.outside == 0 {
            return (target, false);
        }
        if if dir < 0 { y <= 0.0 } else { y >= doc_height } {
            return (target, true);
        }
        y += dir as f64 * 5.0;
    }
}

/// `lineStart(cm, lineN)`.
pub fn line_start(cm: &Editor, line_n: i64) -> Pos {
    let visual = cm.doc.visual_line_no(line_n);
    let text = cm.doc.get_line_utf16(visual).unwrap_or_default();
    end_of_line(cm, true, &text, visual, 1)
}

/// `lineEnd(cm, lineN)`. The JS measures `line` (not the visual end line) but uses
/// the visual end's number; kept.
pub fn line_end(cm: &Editor, line_n: i64) -> Pos {
    let visual_end = cm.doc.visual_line_end_no(line_n);
    let text = cm.doc.get_line_utf16(line_n).unwrap_or_default();
    end_of_line(cm, true, &text, visual_end, -1)
}

/// `line.text.search(/\S/)`.
pub fn search_non_ws(text: &[u16]) -> i64 {
    text.iter()
        .position(|&c| !is_js_space(c))
        .map(|p| p as i64)
        .unwrap_or(-1)
}

/// `lineStartSmart(cm, pos)`.
pub fn line_start_smart(cm: &Editor, pos: Pos) -> Pos {
    let start = line_start(cm, pos.line);
    let text = cm.doc.get_line_utf16(start.line).unwrap_or_default();
    let order = get_order(&text, cm.doc.direction());
    if order.as_ref().is_none_or(|o| o[0].level == 0) {
        let first_non_ws = start.ch.max(search_non_ws(&text));
        let in_ws = pos.line == start.line && pos.ch <= first_non_ws && pos.ch != 0;
        return Pos::with_sticky(
            start.line,
            if in_ws { 0 } else { first_non_ws },
            start.sticky,
        );
    }
    start
}

/// `cm.coordsChar({left, top: cursorCoords(head, "div").top + 5}, "div")`.
pub fn line_side(cm: &Editor, head: Pos, left: f64) -> Pos {
    let top = cm.cursor_coords(head, CoordSys::Doc).top + 5.0;
    cm.coords_char(left, top).pos()
}
