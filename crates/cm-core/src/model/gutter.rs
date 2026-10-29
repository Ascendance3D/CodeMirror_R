//! `setGutterMarker`, `clearGutter` and `lineInfo` (src/model/Doc.js:228-263) on
//! `changeLine` (src/model/changes.js:332). The marker storage and these calls are the
//! data model of issue #36; the gutter columns, their DOM and `gutterClick` are not.

use std::convert::Infallible;

use super::doc::Doc;
use super::line::{GutterMarker, GutterMarkers, LineHandle};
use super::line_class::LineSel;
use crate::operation_group::LineChange;

/// The result of `lineInfo(line)`. `textClass`, `bgClass` and `wrapClass` are the line's
/// properties (`None` is unset or cleared, which JS tells apart as `undefined` and
/// `null`). `gutterClass` is not a key of CM5's object, so it is not a field; read it
/// from the handle. `widgets` waits for line widgets.
#[derive(Clone)]
pub struct LineInfo {
    pub line: i64,
    pub handle: LineHandle,
    pub text: String,
    pub gutter_markers: Option<GutterMarkers>,
    pub text_class: Option<String>,
    pub bg_class: Option<String>,
    pub wrap_class: Option<String>,
}

/// `isEmpty(markers)`: no entry holds a truthy value.
fn all_cleared(markers: &GutterMarkers) -> bool {
    markers.iter().all(|(_, m)| m.is_none())
}

impl Doc {
    /// `setGutterMarker(line, gutterID, value)`; `None` is a falsy value. Returns the
    /// line (`None` for a removed handle). The change is always reported, even when the
    /// marker was already there.
    pub fn set_gutter_marker(
        &self,
        line: impl Into<LineSel>,
        gutter_id: &str,
        value: Option<GutterMarker>,
    ) -> Option<LineHandle> {
        let sel = line.into();
        let done = self.in_op(|| {
            self.change_line(&sel, LineChange::Gutter, |l| {
                let markers = l.gutter_markers.get_or_insert_with(Vec::new);
                match markers.iter_mut().find(|(id, _)| id == gutter_id) {
                    Some(entry) => entry.1 = value.clone(),
                    None => markers.push((gutter_id.to_string(), value.clone())),
                }
                if value.is_none() && all_cleared(markers) {
                    l.gutter_markers = None;
                }
                Ok::<_, Infallible>(true)
            })
        });
        done.unwrap_or_else(|e| match e {})
    }

    /// `clearGutter(gutterID)`: clears the marker of that gutter on every line that has
    /// one, each reported as a gutter change, all in one operation.
    pub fn clear_gutter(&self, gutter_id: &str) {
        self.in_op(|| {
            let (first, count) = (self.first_line(), self.line_count() as i64);
            for line in self.iter_lines(first, first + count) {
                let set = line
                    .borrow()
                    .gutter_markers
                    .as_ref()
                    .is_some_and(|m| m.iter().any(|(id, v)| id == gutter_id && v.is_some()));
                if !set {
                    continue;
                }
                let sel = LineSel::Handle(LineHandle(line));
                let _ = self.change_line(&sel, LineChange::Gutter, |l| {
                    if let Some(markers) = l.gutter_markers.as_mut() {
                        for entry in markers.iter_mut().filter(|(id, _)| id == gutter_id) {
                            entry.1 = None;
                        }
                        if all_cleared(markers) {
                            l.gutter_markers = None;
                        }
                    }
                    Ok::<_, Infallible>(true)
                });
            }
        });
    }

    /// `lineInfo(line)`. A number outside the document gives `None` (no clipping, unlike
    /// `setGutterMarker`), and so does a handle whose line was removed.
    pub fn line_info(&self, line: impl Into<LineSel>) -> Option<LineInfo> {
        let (n, handle) = match line.into() {
            LineSel::No(n) => (n, LineHandle(self.try_line_ref(n).ok()?)),
            LineSel::Handle(h) => (self.line_no(&h.0)?, h),
        };
        let l = handle.0.borrow();
        Some(LineInfo {
            line: n,
            text: String::from_utf16_lossy(&l.text),
            gutter_markers: l.gutter_markers.clone(),
            text_class: l.text_class.clone(),
            bg_class: l.bg_class.clone(),
            wrap_class: l.wrap_class.clone(),
            handle: handle.clone(),
        })
    }
}
