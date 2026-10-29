//! `src/edit/mouse_events.js`, including the internal drag of a selection
//! (`leftButtonStartDrag`); its drop is the `draggingText` branch of `onDrop`
//! (`src/edit/drop_events.js`), done on mouse-up.
//!
//! Not ported: `handleMappedButton` (`LeftClick`-style key bindings), `bidiSimplify`,
//! `gutterClick` events, the context menu; from `drop_events.js`, drops from outside
//! the editor (text or files, `allowDropFileTypes`), `onDragStart`/`onDragOver` and
//! the drag cursor.
//!
//! Mouse coordinates are "div" coordinates of the text area (x includes the line
//! padding, y is document y without the top padding); the renderer converts window
//! pixels with [`Editor::div_from_window`].

use cm_core::Pos;
use cm_core::pos::cmp;
use cm_core::util::misc::{count_column, find_column};

use crate::editor::{Editor, MouseState, sel_mouse};
use crate::measure::PosInfo;

const DOUBLECLICK_DELAY: f64 = 400.0;

/// A mouse event. `button`: 1 left, 2 middle, 3 right (`e_button`). `time` in ms.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MouseEvent {
    pub x: f64,
    pub y: f64,
    pub button: u8,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
    pub time: f64,
}

#[derive(Clone, Copy, Debug)]
struct PastClick {
    time: f64,
    pos: Pos,
    button: u8,
}

impl PastClick {
    fn compare(&self, time: f64, pos: Pos, button: u8) -> bool {
        self.time + DOUBLECLICK_DELAY > time
            && cmp(&pos, &self.pos).is_eq()
            && button == self.button
    }
}

/// `lastClick` / `lastDoubleClick` (module state in the JS).
#[derive(Clone, Debug, Default)]
pub struct ClickTracker {
    last_click: Option<PastClick>,
    last_double: Option<PastClick>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Single,
    Double,
    Triple,
}

impl ClickTracker {
    /// `clickRepeat(pos, button)`.
    fn click_repeat(&mut self, pos: Pos, button: u8, now: f64) -> Repeat {
        if self
            .last_double
            .is_some_and(|c| c.compare(now, pos, button))
        {
            self.last_click = None;
            self.last_double = None;
            Repeat::Triple
        } else if self.last_click.is_some_and(|c| c.compare(now, pos, button)) {
            self.last_double = Some(PastClick {
                time: now,
                pos,
                button,
            });
            self.last_click = None;
            Repeat::Double
        } else {
            self.last_click = Some(PastClick {
                time: now,
                pos,
                button,
            });
            self.last_double = None;
            Repeat::Single
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseUnit {
    Char,
    Word,
    Line,
    Rectangle,
}

/// `configureMouse` result.
#[derive(Clone, Copy, Debug)]
pub struct Behavior {
    pub unit: MouseUnit,
    pub extend: bool,
    pub add_new: bool,
    pub move_on_drag: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum SelectDrag {
    /// `leftButtonSelect` in progress.
    Select {
        start: Pos,
        behavior: Behavior,
        our_index: usize,
        our_range: (Pos, Pos),
        start_sel: Vec<(Pos, Pos)>,
        last_pos: Pos,
    },
    /// `leftButtonStartDrag`: a press inside the selection.
    Drag {
        pos: Pos,
        behavior: Behavior,
        start_x: f64,
        start_y: f64,
        moved: bool,
    },
}

impl Editor {
    /// Window pixels (relative to the editor's top-left) to "div" coordinates.
    pub fn div_from_window(&self, wx: f64, wy: f64) -> (f64, f64) {
        (
            wx - self.gutter_width + self.display.scroll_left,
            wy + self.display.scroll_top - self.metrics.padding_top,
        )
    }

    /// `posFromMouse(cm, e, liberal, forRect)`.
    pub fn pos_from_mouse(&self, x: f64, y: f64, for_rect: bool) -> PosInfo {
        let mut coords = self.coords_char(x, y);
        if for_rect && coords.x_rel > 0.0 {
            let line = self.line_text(coords.line);
            if line.len() as i64 == coords.ch {
                let col_diff = count_column(&line, Some(line.len()), self.options.tab_size, 0, 0)
                    - line.len() as i64;
                let ch = (((x - self.metrics.padding_h) / self.char_width()).round() as i64
                    - col_diff)
                    .max(0);
                coords = PosInfo {
                    line: coords.line,
                    ch,
                    sticky: None,
                    outside: 0,
                    x_rel: 0.0,
                };
            }
        }
        coords
    }

    /// `configureMouse(cm, repeat, event)`.
    fn configure_mouse(&self, repeat: Repeat, e: &MouseEvent) -> Behavior {
        let rect = e.alt;
        let unit = if rect {
            MouseUnit::Rectangle
        } else {
            match repeat {
                Repeat::Single => MouseUnit::Char,
                Repeat::Double => MouseUnit::Word,
                Repeat::Triple => MouseUnit::Line,
            }
        };
        let ext = self.doc.get_extending();
        Behavior {
            unit,
            extend: ext || e.shift,
            add_new: if self.options.mac { e.meta } else { e.ctrl },
            move_on_drag: !(if self.options.mac { e.alt } else { e.ctrl }),
        }
    }

    /// `onMouseDown(e)` for a click in the text area (gutter clicks are ignored,
    /// like `clickInGutter`).
    pub fn mouse_down(&mut self, e: &MouseEvent) {
        self.display.shift = e.shift;
        // eventInWidget: a widget that does not handle mouse events keeps the click.
        if e.x < 0.0 || self.event_in_widget(e.x, e.y) {
            return;
        }
        let info = self.pos_from_mouse(e.x, e.y, false);
        let pos = info.pos();
        let repeat = self.clicks.click_repeat(pos, e.button, e.time);
        if self.mouse.is_some() && e.button == 1 {
            self.mouse_up(None);
        }
        match e.button {
            1 => self.run_op(|cm| cm.left_button_down(info, repeat, e)),
            2 => self.run_op(|cm| cm.doc.extend_selection(pos, None, None)),
            _ => {}
        }
    }

    /// `leftButtonDown(cm, pos, repeat, event)`.
    fn left_button_down(&mut self, info: PosInfo, repeat: Repeat, e: &MouseEvent) {
        let behavior = self.configure_mouse(repeat, e);
        let pos = info.pos();
        let ranges = self.doc.list_selections();
        if self.options.drag_drop && !self.is_read_only() && repeat == Repeat::Single {
            if let Some(c) = ranges
                .iter()
                .find(|r| cmp(&pos, &r.from()).is_ge() && cmp(&pos, &r.to()).is_le())
            {
                if (cmp(&c.from(), &pos).is_lt() || info.x_rel > 0.0)
                    && (cmp(&c.to(), &pos).is_gt() || info.x_rel < 0.0)
                {
                    self.mouse = Some(MouseState {
                        sel: SelectDrag::Drag {
                            pos,
                            behavior,
                            start_x: e.x,
                            start_y: e.y,
                            moved: false,
                        },
                    });
                    return;
                }
            }
        }
        self.left_button_select(e, pos, behavior);
    }

    /// `rangeForUnit(cm, pos, unit)`.
    fn range_for_unit(&self, pos: Pos, unit: MouseUnit) -> (Pos, Pos) {
        match unit {
            MouseUnit::Word => self.find_word_at(pos),
            MouseUnit::Line => (
                Pos::new(pos.line, 0),
                self.doc.clip_pos(Pos::new(pos.line + 1, 0)),
            ),
            _ => (pos, pos),
        }
    }

    /// `leftButtonSelect(cm, event, start, behavior)`.
    fn left_button_select(&mut self, e: &MouseEvent, start: Pos, behavior: Behavior) {
        let sel = self.doc.selection();
        let ranges: Vec<(Pos, Pos)> = sel.ranges.iter().map(|r| (r.anchor, r.head)).collect();
        let contains = |p: Pos| {
            sel.ranges
                .iter()
                .position(|r| cmp(&p, &r.from()).is_ge() && cmp(&p, &r.to()).is_le())
        };
        let (mut our_range, mut our_index): ((Pos, Pos), i64);
        if behavior.add_new && !behavior.extend {
            match contains(start) {
                Some(i) => {
                    our_index = i as i64;
                    our_range = ranges[i];
                }
                None => {
                    our_index = -1;
                    our_range = (start, start);
                }
            }
        } else {
            our_range = (sel.primary().anchor, sel.primary().head);
            our_index = sel.primary as i64;
        }
        let mut start = start;
        if behavior.unit == MouseUnit::Rectangle {
            if !behavior.add_new {
                our_range = (start, start);
            }
            start = self.pos_from_mouse(e.x, e.y, true).pos();
            our_index = -1;
        } else {
            let range = self.range_for_unit(start, behavior.unit);
            our_range = if behavior.extend {
                extend_range(our_range, range.0, Some(range.1), true)
            } else {
                range
            };
        }
        let mut start_sel = ranges.clone();
        if !behavior.add_new {
            our_index = 0;
            self.doc
                .set_selections(&[(our_range.0, Some(our_range.1))], Some(0), sel_mouse());
            start_sel = self
                .doc
                .list_selections()
                .iter()
                .map(|r| (r.anchor, r.head))
                .collect();
        } else if our_index == -1 {
            our_index = ranges.len() as i64;
            let mut rs: Vec<(Pos, Option<Pos>)> =
                ranges.iter().map(|(a, h)| (*a, Some(*h))).collect();
            rs.push((our_range.0, Some(our_range.1)));
            self.doc.set_selections(
                &rs,
                Some(our_index as usize),
                Some(cm_core::SelOptions {
                    scroll: Some(false),
                    origin: Some("*mouse".into()),
                    ..Default::default()
                }),
            );
        } else if ranges.len() > 1
            && ranges[our_index as usize].0 == ranges[our_index as usize].1
            && behavior.unit == MouseUnit::Char
            && !behavior.extend
        {
            let rs: Vec<(Pos, Option<Pos>)> = ranges
                .iter()
                .enumerate()
                .filter(|(i, _)| *i as i64 != our_index)
                .map(|(_, (a, h))| (*a, Some(*h)))
                .collect();
            self.doc.set_selections(
                &rs,
                Some(0),
                Some(cm_core::SelOptions {
                    scroll: Some(false),
                    origin: Some("*mouse".into()),
                    ..Default::default()
                }),
            );
            start_sel = self
                .doc
                .list_selections()
                .iter()
                .map(|r| (r.anchor, r.head))
                .collect();
        } else {
            self.doc.replace_one_selection(
                our_index as usize,
                our_range.0,
                our_range.1,
                sel_mouse(),
            );
        }
        self.mouse = Some(MouseState {
            sel: SelectDrag::Select {
                start,
                behavior,
                our_index: our_index.max(0) as usize,
                our_range,
                start_sel,
                last_pos: start,
            },
        });
    }

    /// Mouse motion with the button held (`extend(e)` / drag tracking).
    pub fn mouse_move(&mut self, e: &MouseEvent) {
        let Some(state) = self.mouse.take() else {
            return;
        };
        match state.sel {
            SelectDrag::Drag {
                pos,
                behavior,
                start_x,
                start_y,
                moved,
            } => {
                let moved = moved || (start_x - e.x).abs() + (start_y - e.y).abs() >= 10.0;
                self.mouse = Some(MouseState {
                    sel: SelectDrag::Drag {
                        pos,
                        behavior,
                        start_x,
                        start_y,
                        moved,
                    },
                });
            }
            SelectDrag::Select {
                start,
                behavior,
                our_index,
                our_range,
                start_sel,
                last_pos,
            } => {
                let cur = self
                    .pos_from_mouse(e.x, e.y, behavior.unit == MouseUnit::Rectangle)
                    .pos();
                let mut last = last_pos;
                if cmp(&cur, &last_pos).is_ne() {
                    last = cur;
                    self.run_op(|cm| {
                        cm.extend_to(cur, start, behavior, our_index, our_range, &start_sel)
                    });
                }
                self.mouse = Some(MouseState {
                    sel: SelectDrag::Select {
                        start,
                        behavior,
                        our_index,
                        our_range,
                        start_sel,
                        last_pos: last,
                    },
                });
            }
        }
    }

    /// `extendTo(pos)` inside `leftButtonSelect`.
    fn extend_to(
        &mut self,
        pos: Pos,
        start: Pos,
        behavior: Behavior,
        our_index: usize,
        our_range: (Pos, Pos),
        start_sel: &[(Pos, Pos)],
    ) {
        if behavior.unit == MouseUnit::Rectangle {
            let tab_size = self.options.tab_size;
            let start_col = count_column(
                &self.line_text(start.line),
                Some(start.ch.max(0) as usize),
                tab_size,
                0,
                0,
            );
            let pos_col = count_column(
                &self.line_text(pos.line),
                Some(pos.ch.max(0) as usize),
                tab_size,
                0,
                0,
            );
            let (left, right) = (start_col.min(pos_col), start_col.max(pos_col));
            let mut ranges: Vec<(Pos, Option<Pos>)> = Vec::new();
            let end = self.last_line().min(start.line.max(pos.line));
            for line in start.line.min(pos.line)..=end {
                let text = self.line_text(line);
                let left_pos = find_column(&text, left, tab_size);
                if left == right {
                    ranges.push((Pos::new(line, left_pos), Some(Pos::new(line, left_pos))));
                } else if text.len() as i64 > left_pos {
                    ranges.push((
                        Pos::new(line, left_pos),
                        Some(Pos::new(line, find_column(&text, right, tab_size))),
                    ));
                }
            }
            if ranges.is_empty() {
                ranges.push((start, Some(start)));
            }
            let mut all: Vec<(Pos, Option<Pos>)> = start_sel[..our_index.min(start_sel.len())]
                .iter()
                .map(|(a, h)| (*a, Some(*h)))
                .collect();
            all.extend(ranges);
            self.doc.set_selections(
                &all,
                Some(our_index),
                Some(cm_core::SelOptions {
                    origin: Some("*mouse".into()),
                    scroll: Some(false),
                    ..Default::default()
                }),
            );
        } else {
            let old = our_range;
            let range = self.range_for_unit(pos, behavior.unit);
            let (anchor, head);
            let old_from = min_pos(&old.0, &old.1);
            let old_to = max_pos(&old.0, &old.1);
            if cmp(&range.0, &old.0).is_gt() {
                head = range.1;
                anchor = min_pos(&old_from, &range.0);
            } else {
                head = range.0;
                anchor = max_pos(&old_to, &range.1);
            }
            let mut rs: Vec<(Pos, Option<Pos>)> =
                start_sel.iter().map(|(a, h)| (*a, Some(*h))).collect();
            let r = (self.doc.clip_pos(anchor), Some(head));
            if our_index < rs.len() {
                rs[our_index] = r;
            } else {
                rs.push(r);
            }
            self.doc.set_selections(&rs, Some(our_index), sel_mouse());
        }
    }

    /// Mouse button release (`done(e)` / `dragEnd`). `e` is `None` to cancel.
    pub fn mouse_up(&mut self, e: Option<&MouseEvent>) {
        let Some(state) = self.mouse.take() else {
            return;
        };
        match state.sel {
            SelectDrag::Select { .. } => {
                self.doc.clear_last_sel_origin();
            }
            SelectDrag::Drag {
                pos,
                behavior,
                moved,
                ..
            } => {
                let e = match e {
                    Some(e) => *e,
                    None => return,
                };
                if !moved {
                    if !behavior.add_new {
                        self.run_op(|cm| cm.doc.extend_selection(pos, None, None));
                        // `extendSelection(cm.doc, pos, null, null, behavior.extend)`
                    }
                } else {
                    // The drop (`onDrop`, normal drop with `draggingText`).
                    let drop = self.pos_from_mouse(e.x, e.y, false).pos();
                    if self.is_read_only() {
                        return;
                    }
                    let inside = self
                        .doc
                        .list_selections()
                        .iter()
                        .any(|r| cmp(&drop, &r.from()).is_ge() && cmp(&drop, &r.to()).is_le());
                    if inside {
                        return;
                    }
                    let text = self.doc.get_selection(None);
                    if text.is_empty() {
                        return;
                    }
                    let copy = !behavior.move_on_drag;
                    self.run_op(|cm| {
                        let selected = if copy {
                            None
                        } else {
                            Some(cm.doc.list_selections())
                        };
                        cm.doc.set_cursor(drop, None);
                        if let Some(sel) = selected {
                            for r in &sel {
                                cm.doc
                                    .replace_range("", r.anchor, Some(r.head), Some("drag"));
                            }
                        }
                        cm.doc
                            .replace_selection(&text, Some("around"), Some("paste"));
                    });
                }
            }
        }
    }

    /// Whether a mouse selection or drag is in progress.
    pub fn mouse_active(&self) -> bool {
        self.mouse.is_some()
    }
}

/// `extendRange(range, head, other, extend)` (src/model/selection_updates.js) on
/// `(anchor, head)` pairs.
pub fn extend_range(range: (Pos, Pos), head: Pos, other: Option<Pos>, extend: bool) -> (Pos, Pos) {
    if extend {
        let mut anchor = range.0;
        let mut head = head;
        if let Some(other) = other {
            let pos_before = cmp(&head, &anchor).is_lt();
            if pos_before != cmp(&other, &anchor).is_lt() {
                anchor = head;
                head = other;
            } else if pos_before != cmp(&head, &other).is_lt() {
                head = other;
            }
        }
        (anchor, head)
    } else {
        (other.unwrap_or(head), head)
    }
}

/// `minPos(a, b)`.
fn min_pos(a: &Pos, b: &Pos) -> Pos {
    if cmp(a, b).is_lt() { *a } else { *b }
}
/// `maxPos(a, b)`.
fn max_pos(a: &Pos, b: &Pos) -> Pos {
    if cmp(a, b).is_lt() { *b } else { *a }
}
