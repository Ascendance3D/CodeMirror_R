//! Character measurement: the box model that stands in for the DOM in
//! `src/measurement/position_measurement.js`.
//!
//! CM5 measures characters by asking the browser for client rects of rendered text
//! nodes. Here every line is laid out on a deterministic grid instead:
//!
//! * a normal character is `char_width` wide;
//! * a tab is a span of `tabSize - col % tabSize` spaces, where `col` counts UTF-16
//!   units the way `buildToken` (src/line/line_data.js) does;
//! * characters matched by the `specialChars` option are drawn as a one-cell
//!   placeholder (`defaultSpecialCharPlaceholder`, a bullet);
//! * extending characters (combining marks, low surrogates) have no width of their
//!   own; `measureCharInner` widens their range back to the base character, so they
//!   share its box;
//! * East Asian wide characters and astral characters (emoji) get their own widths,
//!   because a browser renders them from a fallback font.
//!
//! With a font size whose monospace advance is a multiple of 1/64 px (DejaVu Sans
//! Mono at 32px: 19.265625px), Chromium's layout produces exactly these positions,
//! which is what lets `oracle/cm5-cmd-ops.mjs` compare command results that go through
//! `coordsChar`/`cursorCoords` (vertical motion, page motion, goLineLeft/Right).
//!
//! Gaps: left-to-right only. `cursorCoords` and `coordsCharInner` ignore `getOrder`
//! (no `coordsBidiPart`/`coordsBidiPartWrapped`, no `other` cursor). A line is laid
//! out from its whole text: collapsed ranges and widgets take no part.

use cm_core::pos::Sticky;
use cm_core::util::misc::{find_first, is_extending_char, skip_extending_chars};

/// Font and box metrics, in CSS pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Metrics {
    /// `charWidth(display)`: advance of one monospace cell.
    pub char_width: f64,
    /// `textHeight(display)`: height of one visual line.
    pub text_height: f64,
    /// Horizontal padding of `pre.CodeMirror-line` (`paddingH`, 4px in codemirror.css).
    pub padding_h: f64,
    /// `paddingTop(display)`: `.CodeMirror-lines { padding: 4px 0 }`.
    pub padding_top: f64,
    /// Width of an East Asian wide/fullwidth BMP character.
    pub wide_width: f64,
    /// Width of an astral (surrogate pair) character, e.g. an emoji.
    pub astral_width: f64,
}

impl Metrics {
    /// A grid where wide and astral characters take two cells.
    pub fn monospace(char_width: f64, text_height: f64) -> Metrics {
        Metrics {
            char_width,
            text_height,
            padding_h: 4.0,
            padding_top: 4.0,
            wide_width: 2.0 * char_width,
            astral_width: 2.0 * char_width,
        }
    }
    /// The metrics of the headless oracle editor (`oracle/cm5-cmd-ops.mjs`): DejaVu
    /// Sans Mono 32px, `line-height: 38px`; emoji 40px and CJK 32px wide from the
    /// fallback fonts (measured in headless Chromium, 2026-10-11).
    pub fn oracle() -> Metrics {
        Metrics {
            char_width: 19.265625,
            text_height: 38.0,
            padding_h: 4.0,
            padding_top: 4.0,
            wide_width: 32.0,
            astral_width: 40.0,
        }
    }
    /// A simple 10x20 grid for unit tests.
    pub fn test_grid() -> Metrics {
        Metrics::monospace(10.0, 20.0)
    }
}

/// The default `specialChars` option (src/edit/options.js) minus `\t`, which
/// `buildToken` renders as a tab span.
pub fn is_special_char(c: u16) -> bool {
    matches!(c, 0x00..=0x08 | 0x0a..=0x1f | 0x7f..=0x9f | 0xad | 0x61c | 0x200b | 0x200e | 0x200f | 0x2028 | 0x2029 | 0x202d | 0x202e | 0x2066 | 0x2067 | 0x2069 | 0xfeff | 0xfff9..=0xfffc)
}

/// East Asian Wide / Fullwidth BMP ranges (the common blocks).
pub fn is_wide_char(c: u16) -> bool {
    matches!(c, 0x1100..=0x115f | 0x2e80..=0x303e | 0x3041..=0x33ff | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xa000..=0xa4cf | 0xac00..=0xd7a3 | 0xf900..=0xfaff | 0xfe30..=0xfe4f | 0xff00..=0xff60 | 0xffe0..=0xffe6)
}

/// What one UTF-16 unit is drawn as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    /// Ordinary text.
    Text,
    /// A tab span, `n` cells wide.
    Tab(u32),
    /// A special-char placeholder.
    Special,
    /// The first unit of a surrogate pair.
    Astral,
    /// A wide (CJK) character.
    Wide,
    /// No box of its own: an extending character or the second half of a pair.
    Extend,
}

/// A `{left, right, top, bottom}` box in line-local coordinates (left includes the
/// line's horizontal padding, as CM5's `pre` rect does).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

/// The layout of one line: a box per UTF-16 unit plus the visual-row bottoms
/// (`lineView.measure.heights`).
#[derive(Clone, Debug)]
pub struct LineLayout {
    /// Per unit: the box of the unit's own glyph (zero width for `Extend`).
    pub boxes: Vec<Rect>,
    pub glyphs: Vec<Glyph>,
    /// Bottom of each visual row, line-local.
    pub heights: Vec<f64>,
    /// Row index of each unit.
    pub rows: Vec<u32>,
    /// Right edge of the widest row (for horizontal scrolling).
    pub width: f64,
    pub len: usize,
    pub padding_h: f64,
    /// `node_start[i]`: a text node of the rendered line (`builder.map`) starts at
    /// unit `i`. Tabs and special characters are nodes of their own
    /// (`buildToken`); style spans add boundaries ([`LineLayout::set_style_ends`]).
    /// `measureCharInner` never widens a range across a node boundary.
    pub node_start: Vec<bool>,
}

fn node_starts(glyphs: &[Glyph]) -> Vec<bool> {
    let mut v = vec![false; glyphs.len() + 1];
    v[0] = true;
    for (i, g) in glyphs.iter().enumerate() {
        if matches!(g, Glyph::Tab(_) | Glyph::Special) {
            v[i] = true;
            v[i + 1] = true;
        }
    }
    v
}

/// Classify every unit of a line and compute tab widths (`buildToken` columns).
pub fn classify(text: &[u16], tab_size: i64) -> Vec<Glyph> {
    let mut out = Vec::with_capacity(text.len());
    let mut col: i64 = 0;
    let tab_size = tab_size.max(1);
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c == 0x09 {
            let w = tab_size - col % tab_size;
            out.push(Glyph::Tab(w as u32));
            col += w;
        } else if is_special_char(c) {
            out.push(Glyph::Special);
            col += 1;
        } else if (0xd800..0xdc00).contains(&c)
            && i + 1 < text.len()
            && (0xdc00..0xe000).contains(&text[i + 1])
        {
            out.push(Glyph::Astral);
            out.push(Glyph::Extend);
            col += 2;
            i += 2;
            continue;
        } else if (0xdc00..0xe000).contains(&c) {
            // A lone low surrogate is drawn as a one-cell replacement glyph, but
            // `isExtendingChar` still merges it into the previous character's range
            // (probed in headless Chromium, 2026-10-11).
            out.push(Glyph::Text);
            col += 1;
        } else if is_extending_char(c) {
            // Extending characters have no advance, even at the start of a line
            // (Chromium draws a leading U+0301 as a zero-width box; probed 2026-10-11).
            out.push(Glyph::Extend);
            col += 1;
        } else if is_wide_char(c) {
            out.push(Glyph::Wide);
            col += 1;
        } else {
            out.push(Glyph::Text);
            col += 1;
        }
        i += 1;
    }
    out
}

fn glyph_width(m: &Metrics, g: Glyph) -> f64 {
    match g {
        Glyph::Text | Glyph::Special => m.char_width,
        Glyph::Tab(n) => n as f64 * m.char_width,
        Glyph::Astral => m.astral_width,
        Glyph::Wide => m.wide_width,
        Glyph::Extend => 0.0,
    }
}

fn is_break_space(c: u16) -> bool {
    c == 0x20 || c == 0x09
}

impl LineLayout {
    /// Lay out `text`. `wrap_width` is the content width available for
    /// `white-space: pre-wrap; word-wrap: break-word` wrapping (`None` = no wrapping).
    pub fn new(m: &Metrics, text: &[u16], tab_size: i64, wrap_width: Option<f64>) -> LineLayout {
        let glyphs = classify(text, tab_size);
        let n = text.len();
        let mut boxes = Vec::with_capacity(n);
        let mut rows = Vec::with_capacity(n);
        let th = m.text_height;
        match wrap_width {
            None => {
                let mut x = m.padding_h;
                for &g in &glyphs {
                    let w = glyph_width(m, g);
                    boxes.push(Rect {
                        left: x,
                        right: x + w,
                        top: 0.0,
                        bottom: th,
                    });
                    rows.push(0);
                    x += w;
                }
                LineLayout {
                    node_start: node_starts(&glyphs),
                    boxes,
                    glyphs,
                    heights: vec![th],
                    rows,
                    width: x + m.padding_h,
                    len: n,
                    padding_h: m.padding_h,
                }
            }
            Some(avail) => {
                // Greedy pre-wrap: break opportunities after runs of spaces/tabs (which
                // hang at the end of a row); words wider than a row break anywhere.
                let avail = avail.max(m.char_width);
                let mut x = 0.0f64;
                let mut row = 0u32;
                let mut maxw = 0.0f64;
                let mut i = 0;
                let widths: Vec<f64> = glyphs.iter().map(|&g| glyph_width(m, g)).collect();
                boxes.resize(
                    n,
                    Rect {
                        left: 0.0,
                        right: 0.0,
                        top: 0.0,
                        bottom: 0.0,
                    },
                );
                rows.resize(n, 0);
                while i < n {
                    // A word: non-space units, then the spaces after it.
                    let mut j = i;
                    while j < n && !is_break_space(text[j]) {
                        j += 1;
                    }
                    let word_w: f64 = widths[i..j].iter().sum();
                    if x > 0.0 && x + word_w > avail + 1e-9 {
                        row += 1;
                        x = 0.0;
                    }
                    for k in i..j {
                        let w = widths[k];
                        if x > 0.0 && w > 0.0 && x + w > avail + 1e-9 {
                            row += 1;
                            x = 0.0;
                        }
                        boxes[k] = Rect {
                            left: m.padding_h + x,
                            right: m.padding_h + x + w,
                            top: 0.0,
                            bottom: 0.0,
                        };
                        rows[k] = row;
                        x += w;
                        maxw = maxw.max(x);
                    }
                    let mut k = j;
                    while k < n && is_break_space(text[k]) {
                        let w = widths[k];
                        boxes[k] = Rect {
                            left: m.padding_h + x,
                            right: m.padding_h + x + w,
                            top: 0.0,
                            bottom: 0.0,
                        };
                        rows[k] = row;
                        x += w;
                        k += 1;
                    }
                    i = k;
                }
                let nrows = row as usize + 1;
                let heights: Vec<f64> = (1..=nrows).map(|r| r as f64 * th).collect();
                for k in 0..n {
                    let r = rows[k] as f64;
                    boxes[k].top = r * th;
                    boxes[k].bottom = (r + 1.0) * th;
                }
                LineLayout {
                    node_start: node_starts(&glyphs),
                    boxes,
                    glyphs,
                    heights,
                    rows,
                    width: maxw + 2.0 * m.padding_h,
                    len: n,
                    padding_h: m.padding_h,
                }
            }
        }
    }

    /// Height of the whole line.
    pub fn height(&self) -> f64 {
        *self.heights.last().unwrap()
    }

    /// The `[start, end)` cluster `measureCharInner` measures for `ch`: back over
    /// extending characters to the base, forward over the base's extenders.
    fn cluster(&self, text: &[u16], ch: usize) -> (usize, usize) {
        let mut start = ch;
        let mut end = ch + 1;
        while start > 0 && !self.node_start[start] && is_extending_char(text[start]) {
            start -= 1;
        }
        while end < self.len && !self.node_start[end] && is_extending_char(text[end]) {
            end += 1;
        }
        (start, end)
    }

    /// Add the text-node boundaries of a style array (`(end, style)` spans, as
    /// `insertLineContent` turns each span into its own node).
    pub fn set_style_ends(&mut self, ends: impl IntoIterator<Item = usize>) {
        for e in ends {
            if e <= self.len {
                self.node_start[e] = true;
            }
        }
    }

    fn cluster_rect(&self, text: &[u16], ch: usize) -> Rect {
        let (s, e) = self.cluster(text, ch);
        let mut r = self.boxes[s];
        // A range spanning several boxes on one row: its union (rows never split a
        // cluster in this layout).
        for k in s + 1..e {
            r.right = r.right.max(self.boxes[k].right);
        }
        r
    }

    /// `measureCharPrepared(cm, prepared, ch, bias)` (src/measurement/position_measurement.js)
    /// via `nodeAndOffsetInLineMap` + `measureCharInner`, for a line that is a single
    /// text node: positions past the end collapse to the right edge of the last
    /// character, positions before the start to the left edge of the first.
    pub fn measure_char(&self, text: &[u16], ch: i64) -> Rect {
        if self.len == 0 {
            // `zeroWidthElement`: a widget node at the line start.
            return Rect {
                left: self.padding_h,
                right: self.padding_h,
                top: 0.0,
                bottom: self.heights[0],
            };
        }
        if ch < 0 {
            // `ch < mStart`: start = 0, end = 1, collapse "left".
            let r = self.cluster_rect(text, 0);
            return Rect {
                left: r.left,
                right: r.left,
                ..r
            };
        }
        let ch = ch as usize;
        if ch >= self.len {
            // Last node, `ch >= mEnd`: end = len, start = end - 1, collapse "right".
            let r = self.cluster_rect(text, self.len - 1);
            return Rect {
                left: r.right,
                right: r.right,
                ..r
            };
        }
        self.cluster_rect(text, ch)
    }

    /// `cursorCoords` in "line" coordinates (no bidi): a zero-width box.
    pub fn cursor_coords(&self, text: &[u16], ch: i64, sticky: Option<Sticky>) -> Rect {
        let (mut ch, mut sticky) = (ch, sticky);
        if ch >= self.len as i64 {
            ch = self.len as i64;
            sticky = Some(Sticky::Before);
        } else if ch <= 0 {
            ch = 0;
            sticky = Some(Sticky::After);
        }
        // `get(sticky == "before" ? ch - 1 : ch, sticky == "before")`
        let right = sticky == Some(Sticky::Before);
        let m = self.measure_char(text, if right { ch - 1 } else { ch });
        let x = if right { m.right } else { m.left };
        Rect {
            left: x,
            right: x,
            top: m.top,
            bottom: m.bottom,
        }
    }

    /// `wrappedLineExtent(cm, lineObj, preparedMeasure, y)`.
    pub fn wrapped_line_extent(&self, text: &[u16], y: f64) -> (i64, i64) {
        let end = self.len as i64;
        let begin = find_first(|ch| self.measure_char(text, ch - 1).bottom <= y, end, 0);
        let end = find_first(|ch| self.measure_char(text, ch).top > y, begin, end);
        (begin, end)
    }

    /// `wrappedLineExtentChar(cm, lineObj, preparedMeasure, target)`.
    pub fn wrapped_line_extent_char(&self, text: &[u16], target: i64) -> (i64, i64) {
        let top = self.measure_char(text, target).top;
        self.wrapped_line_extent(text, top)
    }
}

/// `boxIsAfter(box, x, y, left)`.
fn box_is_after(b: &Rect, x: f64, y: f64, left: bool) -> bool {
    if b.bottom <= y {
        false
    } else if b.top > y {
        true
    } else {
        (if left { b.left } else { b.right }) > x
    }
}

/// A `coordsChar` result: `PosWithInfo(line, ch, sticky, outside, xRel)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PosInfo {
    pub line: i64,
    pub ch: i64,
    pub sticky: Option<Sticky>,
    /// -1 above, 1 below, 0 inside the line's vertical range.
    pub outside: i8,
    pub x_rel: f64,
}

impl PosInfo {
    /// The position part (`Pos(line, ch, sticky)`).
    pub fn pos(&self) -> cm_core::Pos {
        cm_core::Pos::with_sticky(self.line, self.ch, self.sticky)
    }
}

/// `coordsCharInner(cm, lineObj, lineNo, x, y)` for a left-to-right line, with `y`
/// already line-local.
pub fn coords_char_inner(
    layout: &LineLayout,
    text: &[u16],
    line_no: i64,
    x: f64,
    y: f64,
) -> PosInfo {
    let begin = 0i64;
    let end = layout.len as i64;
    let ltr = true;
    let mut ch_around: Option<i64> = None;
    let mut box_around: Option<Rect> = None;
    let mut ch = find_first(
        |ch| {
            let b = layout.measure_char(text, ch);
            if !box_is_after(&b, x, y, false) {
                return false;
            }
            if b.top <= y && b.left <= x {
                ch_around = Some(ch);
                box_around = Some(b);
            }
            true
        },
        begin,
        end,
    );
    let base_x;
    let sticky;
    let mut outside = 0i8;
    if let (Some(ca), Some(ba)) = (ch_around, box_around) {
        let at_left = x - ba.left < ba.right - x;
        let at_start = at_left == ltr;
        ch = ca + if at_start { 0 } else { 1 };
        sticky = if at_start {
            Sticky::After
        } else {
            Sticky::Before
        };
        base_x = if at_left { ba.left } else { ba.right };
    } else {
        sticky = if ch == 0 {
            Sticky::After
        } else if ch == layout.len as i64 {
            Sticky::Before
        } else if (layout
            .measure_char(text, ch - if ltr { 1 } else { 0 })
            .bottom
            <= y)
            == ltr
        {
            Sticky::After
        } else {
            Sticky::Before
        };
        let coords = layout.cursor_coords(text, ch, Some(sticky));
        base_x = coords.left;
        outside = if y < coords.top {
            -1
        } else if y >= coords.bottom {
            1
        } else {
            0
        };
    }
    let ch = skip_extending_chars(text, ch, 1);
    PosInfo {
        line: line_no,
        ch,
        sticky: Some(sticky),
        outside,
        x_rel: x - base_x,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cm_core::text::to_u16;

    #[test]
    fn oracle_positions_match_chromium_probe() {
        // Values read from headless Chromium (CM5, DejaVu Sans Mono 32px, tabSize 4)
        // for "\tab\té́z😀q中w".
        let m = Metrics::oracle();
        let t = to_u16("\tab\té\u{301}z😀q中w");
        let l = LineLayout::new(&m, &t, 4, None);
        let got: Vec<f64> = (0..=13)
            .map(|i| l.cursor_coords(&t, i, None).left)
            .collect();
        let want = [
            4.0, 81.0625, 100.328125, 119.59375, 158.125, 158.125, 177.390625, 196.65625,
            196.65625, 236.65625, 255.921875, 287.921875, 307.1875, 307.1875,
        ];
        assert_eq!(got, want);
    }
}
