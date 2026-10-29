//! `addLineClass`/`removeLineClass` (src/model/Doc.js:265-291) and `changeLine`
//! (src/model/changes.js:332-340).

use super::doc::Doc;
use super::line::{Line, LineHandle};
use crate::operation_group::LineChange;
use crate::util::regexp::{RegExp, RegExpError};

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

/// `classTest(cls)` (src/util/dom.js:3): `new RegExp("(^|\\s)" + cls + "(?:$|\\s)\\s*")`.
/// `cls` is spliced in unescaped, so regular-expression syntax in it keeps its meaning
/// and an invalid pattern is the `SyntaxError` the JS constructor throws.
fn class_test(cls: &str) -> Result<RegExp, RegExpError> {
    RegExp::new(&format!("(^|\\s){cls}(?:$|\\s)\\s*"), "")
}

/// `addLineClass`'s `op`. The expression is built only for a non-empty property, as
/// in CM5 (`if (!line[prop]) line[prop] = cls`), so an empty property accepts any
/// `cls`, even one that is not a valid pattern.
fn add_class(slot: &mut Option<String>, cls: &str) -> Result<bool, RegExpError> {
    match slot.as_deref() {
        None | Some("") => *slot = Some(cls.to_string()),
        Some(cur) => {
            if class_test(cls)?.test_str(cur) {
                return Ok(false);
            }
            *slot = Some(format!("{cur} {cls}"));
        }
    }
    Ok(true)
}

/// `removeLineClass`'s `op`; `cls` of `None` clears the property. Indices are UTF-16
/// code units, as in JS.
fn remove_class(slot: &mut Option<String>, cls: Option<&str>) -> Result<bool, RegExpError> {
    let Some(cur) = slot.as_deref().filter(|c| !c.is_empty()) else {
        return Ok(false);
    };
    let Some(cls) = cls else {
        *slot = None;
        return Ok(true);
    };
    let units: Vec<u16> = cur.encode_utf16().collect();
    let Some(found) = class_test(cls)?.exec(&units) else {
        return Ok(false);
    };
    let (start, end) = (found.index(), found.end());
    let sep: &[u16] = if start == 0 || end == units.len() {
        &[]
    } else {
        &[b' ' as u16]
    };
    let mut next = units[..start].to_vec();
    next.extend_from_slice(sep);
    next.extend_from_slice(&units[end..]);
    *slot = (!next.is_empty()).then(|| String::from_utf16_lossy(&next));
    Ok(true)
}

impl Doc {
    /// `changeLine(doc, handle, changeType, op)`: `op` returns whether it changed the
    /// line, and then the editor redraws it. Returns `None` for a handle whose line
    /// was removed.
    pub(crate) fn change_line(
        &self,
        sel: &LineSel,
        kind: LineChange,
        op: impl FnOnce(&mut Line) -> Result<bool, RegExpError>,
    ) -> Result<Option<LineHandle>, RegExpError> {
        let (line, no) = match sel {
            // The property is the clipped line's, but `changeLine` keeps the number it was
            // given as `no` (`let no = handle`), so `regLineChange` gets the unclipped one:
            // a line number outside the document redraws nothing.
            LineSel::No(n) => {
                let clipped = (*n).min(self.last_line()).max(self.first_line());
                (self.get_line_ref(clipped), *n)
            }
            LineSel::Handle(h) => match self.line_no(&h.0) {
                Some(no) => (h.0.clone(), no),
                None => return Ok(None),
            },
        };
        let changed = op(&mut line.borrow_mut())?;
        if changed && let Some(sink) = self.sink() {
            sink.line_changed(self, no, kind);
        }
        Ok(Some(LineHandle(line)))
    }

    /// `addLineClass(handle, where, cls)`. Returns the line, even when it already had
    /// the class; `Ok(None)` for a removed line. `Err` is the `SyntaxError` CM5 throws
    /// when `cls` is not a valid regular expression and the property is not empty; the
    /// line is then unchanged and not redrawn.
    pub fn add_line_class(
        &self,
        line: impl Into<LineSel>,
        place: LineWhere,
        cls: &str,
    ) -> Result<Option<LineHandle>, RegExpError> {
        let sel = line.into();
        self.in_op(|| self.change_line(&sel, kind_of(place), |l| add_class(place.slot(l), cls)))
    }

    /// `removeLineClass(handle, where, cls)`; `cls` of `None` clears the property.
    pub fn remove_line_class(
        &self,
        line: impl Into<LineSel>,
        place: LineWhere,
        cls: Option<&str>,
    ) -> Result<Option<LineHandle>, RegExpError> {
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
