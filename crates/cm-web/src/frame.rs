//! What one browser frame shows, read from the shared editor: the rendered line
//! range with its token spans, the selection and cursor boxes, and the scroll
//! extents. The DOM layer ([`crate::dom`]) only writes these values into
//! elements; it keeps no document of its own.
//!
//! Coordinates are CM5's "div" system (`display.lineDiv`): x includes the line's
//! `paddingH`, y is the document offset without `paddingTop`. Lines are laid out
//! by `cm_view::measure` (the box model that stands in for DOM measurement), so
//! the browser text must be set in the font whose metrics the editor holds
//! ([`crate::dom`] measures it).

use std::rc::Rc;

use cm_core::Sticky;
use cm_core::text::Text;
use cm_view::highlight::Styles;
use cm_view::measure::{Glyph, Rect, classify};
use cm_view::{CoordSys, Editor};

/// One piece of a rendered line (`buildToken`, src/line/line_data.js).
#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    /// Text with its token classes (`cm-keyword cm-def`), if any.
    Text { text: String, class: Option<String> },
    /// A tab drawn as `n` spaces (`span.cm-tab`).
    Tab { n: u32, class: Option<String> },
    /// A special character drawn as a placeholder (`span.cm-invalidchar`).
    Special { code: u16, class: Option<String> },
}

/// The class strings of a rendered line (`updateLineClasses`, `updateLineBackground`):
/// the mode's `line-*` classes first, then those `addLineClass` set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineClasses {
    /// The class of the `pre.CodeMirror-line`.
    pub text: Option<String>,
    /// The class of the `div.CodeMirror-linebackground`, without that base class.
    pub bg: Option<String>,
    /// The class of the line's wrapper.
    pub wrap: Option<String>,
}

/// `lineView.textClass + " " + line.textClass`, or whichever is set.
fn join_classes(mode: Option<&String>, line: Option<String>) -> Option<String> {
    let line = line.filter(|c| !c.is_empty());
    match (mode.filter(|c| !c.is_empty()), line) {
        (Some(m), Some(l)) => Some(format!("{m} {l}")),
        (Some(m), None) => Some(m.clone()),
        (None, l) => l,
    }
}

impl LineClasses {
    pub fn of(styles: &Styles, handle: &cm_core::LineHandle) -> LineClasses {
        let (mode_bg, mode_text) = styles
            .classes
            .as_ref()
            .map_or((None, None), |(b, t)| (b.as_ref(), t.as_ref()));
        LineClasses {
            text: join_classes(mode_text, handle.text_class()),
            bg: join_classes(mode_bg, handle.bg_class()),
            wrap: handle.wrap_class().filter(|c| !c.is_empty()),
        }
    }
}

/// One rendered line.
#[derive(Clone, Debug)]
pub struct FrameLine {
    /// The line handle's id: stable while the line object lives.
    pub id: usize,
    pub line: i64,
    /// The gutter label (`lineNumberFor`), when line numbers are on.
    pub number: Option<String>,
    /// Document y of the line's top ("div" coordinates).
    pub top: f64,
    pub height: f64,
    pub text: Text,
    /// The style array the segments come from (pointer-compared for reuse).
    pub styles: Rc<Styles>,
    pub classes: LineClasses,
}

/// A frame of the editor.
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: f64,
    pub height: f64,
    pub gutter_width: f64,
    pub padding_top: f64,
    pub text_height: f64,
    pub scroll_top: f64,
    pub scroll_left: f64,
    /// Height of the scrollable content (`sizer`): document plus vertical padding.
    pub content_height: f64,
    /// Width of the text content (the widest line), 0 while lines wrap.
    pub content_width: f64,
    /// Wrap width of `pre.CodeMirror-line` content while lines wrap.
    pub wrap_width: Option<f64>,
    pub lines: Vec<FrameLine>,
    pub selections: Vec<Rect>,
    pub cursors: Vec<Rect>,
    pub focused: bool,
    pub overwrite: bool,
}

fn token_class(style: Option<&str>) -> Option<String> {
    let s = style?.split_whitespace().collect::<Vec<_>>();
    if s.is_empty() {
        return None;
    }
    Some(
        s.iter()
            .map(|w| format!("cm-{w}"))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// The segments of a line from its text and style array, cut where styles change
/// and around tabs and special characters.
pub fn line_segments(text: &[u16], styles: &Styles, tab_size: i64) -> Vec<Seg> {
    let glyphs = classify(text, tab_size);
    let mut out = Vec::new();
    let mut start = 0usize;
    for (end, style) in &styles.spans {
        let end = (*end).min(text.len());
        if end <= start {
            continue;
        }
        let class = token_class(style.as_deref());
        let mut run = start;
        let mut i = start;
        let flush = |out: &mut Vec<Seg>, a: usize, b: usize| {
            if b > a {
                out.push(Seg::Text {
                    text: String::from_utf16_lossy(&text[a..b]),
                    class: class.clone(),
                });
            }
        };
        while i < end {
            match glyphs[i] {
                Glyph::Tab(n) => {
                    flush(&mut out, run, i);
                    out.push(Seg::Tab {
                        n,
                        class: class.clone(),
                    });
                    run = i + 1;
                }
                Glyph::Special => {
                    flush(&mut out, run, i);
                    out.push(Seg::Special {
                        code: text[i],
                        class: class.clone(),
                    });
                    run = i + 1;
                }
                _ => {}
            }
            i += 1;
        }
        flush(&mut out, run, end);
        start = end;
    }
    if start < text.len() {
        out.push(Seg::Text {
            text: String::from_utf16_lossy(&text[start..]),
            class: None,
        });
    }
    out
}

/// The current frame of `editor`. `focused` comes from the host (`state.focused`).
pub fn build(editor: &Editor, focused: bool) -> Frame {
    let m = &editor.metrics;
    let first = editor.first_line();
    let end = first + editor.doc.line_count() as i64;
    // The rendered range (`display.viewFrom`/`viewTo`), at least the visible lines.
    let (vis_from, vis_to) = editor.visible_lines();
    let from = editor.display.view_from.min(vis_from).max(first);
    let to = editor.display.view_to.max(vis_to + 1).min(end);
    let wrap = editor.options.line_wrapping;
    let content_width = if wrap { 0.0 } else { editor.max_line_width() };

    let mut lines = Vec::new();
    for line in from..to {
        let Some(handle) = editor.doc.get_line_handle(line) else {
            continue;
        };
        let number = editor
            .options
            .line_numbers
            .then(|| (line + editor.options.first_line_number - first).to_string());
        let styles = editor.line_styles(line);
        lines.push(FrameLine {
            id: handle.id(),
            line,
            number,
            top: editor.height_at_line(line),
            height: editor.line_height(line),
            text: handle.text_utf16(),
            classes: LineClasses::of(&styles, &handle),
            styles,
        });
    }

    // `drawSelectionRange`: right side is the sizer or the display, minus padding.
    let right = content_width.max(editor.text_area_width()) - m.padding_h;
    let left = m.padding_h;
    let sel = editor.doc.selection();
    let mut selections = Vec::new();
    let boxed = |l: f64, t: f64, r: f64, b: f64| Rect {
        left: l,
        top: t,
        right: r.max(l),
        bottom: b,
    };
    for r in &sel.ranges {
        if r.empty() {
            continue;
        }
        let (a, b) = (r.from(), r.to());
        for line in a.line.max(from)..=b.line.min(to - 1) {
            let (t, l) = editor.layout(line);
            let top = editor.height_at_line(line);
            let s_ch = if line == a.line { a.ch } else { 0 };
            let e_ch = if line == b.line { b.ch } else { t.len() as i64 };
            let lc = l.cursor_coords(&t, s_ch, Some(Sticky::After));
            let rc = l.cursor_coords(&t, e_ch, Some(Sticky::Before));
            let extend = line != b.line;
            let x1 = |c: &Rect| if extend { right } else { c.left };
            if lc.top == rc.top {
                selections.push(boxed(lc.left, top + lc.top, x1(&rc), top + lc.bottom));
            } else {
                selections.push(boxed(lc.left, top + lc.top, right, top + lc.bottom));
                if rc.top > lc.bottom {
                    selections.push(boxed(left, top + lc.bottom, right, top + rc.top));
                }
                selections.push(boxed(left, top + rc.top, x1(&rc), top + rc.bottom));
            }
        }
    }

    // `drawSelectionCursor` for empty ranges (`showCursorWhenSelecting` false).
    let mut cursors = Vec::new();
    for r in &sel.ranges {
        if !r.empty() || r.head.line < from || r.head.line >= to {
            continue;
        }
        cursors.push(editor.cursor_coords(r.head, CoordSys::Doc));
    }

    Frame {
        width: editor.display.width,
        height: editor.display.height,
        gutter_width: editor.gutter_width,
        padding_top: m.padding_top,
        text_height: m.text_height,
        scroll_top: editor.display.scroll_top,
        scroll_left: editor.display.scroll_left,
        content_height: editor.doc_height() + 2.0 * m.padding_top,
        content_width,
        wrap_width: wrap.then(|| editor.wrap_width()),
        lines,
        selections,
        cursors,
        focused,
        overwrite: editor.overwrite,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cm_core::Pos;
    use cm_modes::Spec;
    use cm_view::{EditorOptions, Metrics};

    fn ed(text: &str, mode: &str) -> Editor {
        Editor::new(
            text,
            Spec::from(mode),
            EditorOptions::default(),
            Metrics::test_grid(),
        )
    }

    #[test]
    fn segments_follow_styles_tabs_and_specials() {
        let cm = ed("int\tx = 1; // c\u{7}", "text/x-csrc");
        cm.catch_up_states(None, true);
        let t = cm.line_text(0);
        let segs = line_segments(&t, &cm.line_styles(0), 4);
        let text: String = segs
            .iter()
            .map(|s| match s {
                Seg::Text { text, .. } => text.clone(),
                Seg::Tab { n, .. } => " ".repeat(*n as usize),
                Seg::Special { .. } => "\u{2022}".into(),
            })
            .collect();
        assert_eq!(text, "int x = 1; // c\u{2022}");
        assert_eq!(
            segs[0],
            Seg::Text {
                text: "int".into(),
                class: Some("cm-type".into())
            }
        );
        assert_eq!(segs[1], Seg::Tab { n: 1, class: None });
        assert!(
            matches!(&segs[segs.len() - 1], Seg::Special { code: 7, class } if class.as_deref() == Some("cm-comment"))
        );
        // The token classes equal `getLineTokens`.
        let toks = cm.get_line_tokens(0, true);
        assert_eq!(toks.last().unwrap().ty.as_deref(), Some("comment"));
    }

    #[test]
    fn frame_geometry() {
        let mut cm = ed("abc\ndef\nghi", "text/plain");
        cm.set_size(200.0, 100.0);
        cm.doc
            .set_selection(Pos::new(0, 1), Some(Pos::new(1, 2)), None);
        let f = build(&cm, true);
        assert_eq!(f.lines.len(), 3);
        assert_eq!(f.lines[1].top, 20.0);
        assert_eq!(f.content_height, 3.0 * 20.0 + 8.0);
        // Line 0 from ch 1 to the right side; line 1 to ch 2.
        assert_eq!(f.selections.len(), 2);
        assert_eq!(f.selections[0].left, 4.0 + 10.0);
        assert_eq!(f.selections[1].right, 4.0 + 20.0);
        assert!(f.cursors.is_empty());
        cm.doc.set_cursor(Pos::new(2, 3), None);
        let f = build(&cm, true);
        assert_eq!(f.cursors.len(), 1);
        assert_eq!((f.cursors[0].left, f.cursors[0].top), (34.0, 40.0));
    }
}

#[cfg(test)]
mod line_class_tests {
    use super::*;

    #[test]
    fn mode_classes_come_before_line_classes() {
        let a = Some("m".to_string());
        assert_eq!(
            join_classes(a.as_ref(), Some("l".into())).as_deref(),
            Some("m l")
        );
        assert_eq!(join_classes(a.as_ref(), None).as_deref(), Some("m"));
        assert_eq!(join_classes(None, Some("l".into())).as_deref(), Some("l"));
        assert_eq!(
            join_classes(Some(&String::new()), Some(String::new())),
            None
        );
    }
}
