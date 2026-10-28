//! `addLineClass`/`removeLineClass` (src/model/Doc.js:265-291) and `changeLine`
//! (src/model/changes.js:332-340).

use super::doc::Doc;
use super::line::{Line, LineHandle};
use crate::operation_group::LineChange;

/// The `where` argument. CM5 maps `"text"`, `"background"` and `"gutter"` to their
/// property and every other string to `wrapClass`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineWhere {
    Text,
    Background,
    Wrap,
    Gutter,
}

impl LineWhere {
    /// The CM5 string form: anything but `text`, `background` and `gutter` is `wrap`.
    pub fn parse(s: &str) -> LineWhere {
        match s {
            "text" => LineWhere::Text,
            "background" => LineWhere::Background,
            "gutter" => LineWhere::Gutter,
            _ => LineWhere::Wrap,
        }
    }
    fn slot(self, line: &mut Line) -> &mut Option<String> {
        match self {
            LineWhere::Text => &mut line.text_class,
            LineWhere::Background => &mut line.bg_class,
            LineWhere::Wrap => &mut line.wrap_class,
            LineWhere::Gutter => &mut line.gutter_class,
        }
    }
}

/// A line argument: a line number (clipped into the document) or a handle.
#[derive(Clone)]
pub enum LineSel {
    No(i64),
    Handle(LineHandle),
}

impl From<i64> for LineSel {
    fn from(n: i64) -> Self {
        LineSel::No(n)
    }
}

impl From<&LineHandle> for LineSel {
    fn from(h: &LineHandle) -> Self {
        LineSel::Handle(h.clone())
    }
}

/// The match of `new RegExp("(^|\\s)" + cls + "(?:$|\\s)\\s*")` in `s`, as a byte
/// range. `cls` is matched literally; CM5 builds the expression unescaped, which only
/// differs for a class name with regular-expression syntax in it.
fn class_match(s: &str, cls: &str) -> Option<(usize, usize)> {
    let mut from = 0;
    while let Some(p) = s[from..].find(cls) {
        let i = from + p;
        let end = i + cls.len();
        let before = s[..i].chars().next_back();
        let after = s[end..].chars().next();
        if before.is_none_or(char::is_whitespace) && after.is_none_or(char::is_whitespace) {
            let start = i - before.map_or(0, char::len_utf8);
            let tail = s[end..].len() - s[end..].trim_start().len();
            return Some((start, end + tail));
        }
        from = i + s[i..].chars().next().map_or(1, char::len_utf8);
    }
    None
}

fn add_class(slot: &mut Option<String>, cls: &str) -> bool {
    match slot.as_deref() {
        None | Some("") => *slot = Some(cls.to_string()),
        Some(cur) if class_match(cur, cls).is_some() => return false,
        Some(cur) => *slot = Some(format!("{cur} {cls}")),
    }
    true
}

fn remove_class(slot: &mut Option<String>, cls: Option<&str>) -> bool {
    let Some(cur) = slot.as_deref().filter(|c| !c.is_empty()) else {
        return false;
    };
    let Some(cls) = cls else {
        *slot = None;
        return true;
    };
    let Some((start, end)) = class_match(cur, cls) else {
        return false;
    };
    let sep = if start == 0 || end == cur.len() {
        ""
    } else {
        " "
    };
    let next = format!("{}{sep}{}", &cur[..start], &cur[end..]);
    *slot = (!next.is_empty()).then_some(next);
    true
}

impl Doc {
    /// `changeLine(doc, handle, changeType, op)`: `op` returns whether it changed the
    /// line, and then the editor redraws it. Returns `None` for a handle whose line
    /// was removed.
    pub(crate) fn change_line(
        &self,
        sel: &LineSel,
        kind: LineChange,
        op: impl FnOnce(&mut Line) -> bool,
    ) -> Option<LineHandle> {
        let (line, no) = match sel {
            LineSel::No(n) => {
                let n = (*n).min(self.last_line()).max(self.first_line());
                (self.get_line_ref(n), n)
            }
            LineSel::Handle(h) => (h.0.clone(), self.line_no(&h.0)?),
        };
        let changed = op(&mut line.borrow_mut());
        if changed && let Some(sink) = self.sink() {
            sink.line_changed(self, no, kind);
        }
        Some(LineHandle(line))
    }

    /// `addLineClass(handle, where, cls)`. Returns the line, even when it already had
    /// the class; `None` for a removed line.
    pub fn add_line_class(
        &self,
        line: impl Into<LineSel>,
        place: LineWhere,
        cls: &str,
    ) -> Option<LineHandle> {
        let sel = line.into();
        self.in_op(|| self.change_line(&sel, kind_of(place), |l| add_class(place.slot(l), cls)))
    }

    /// `removeLineClass(handle, where, cls)`; `cls` of `None` clears the property.
    pub fn remove_line_class(
        &self,
        line: impl Into<LineSel>,
        place: LineWhere,
        cls: Option<&str>,
    ) -> Option<LineHandle> {
        let sel = line.into();
        self.in_op(|| self.change_line(&sel, kind_of(place), |l| remove_class(place.slot(l), cls)))
    }
}

fn kind_of(place: LineWhere) -> LineChange {
    if place == LineWhere::Gutter {
        LineChange::Gutter
    } else {
        LineChange::Class
    }
}
