//! `src/line/highlight.js` and `src/display/highlight_worker.js`: running the mode
//! over lines, the per-line state cache (`line.stateAfter`), the mode and highlight
//! frontiers, style arrays, and `getTokenAt`.
//!
//! The cached values live on cm-core's line objects ([`LineHandle::state_after`],
//! [`LineHandle::styles`]), which `updateLine` clears exactly like the JS.
//!
//! `runMode` honors `addModeClass` and a mode's own `flattenSpans`.
//!
//! Not ported: overlays (`addOverlay`, `cm.state.overlays`, `baseToken`), so
//! `highlightLine` runs the mode alone. Mode line classes (`line-*` /
//! `line-background-*` tokens) are collected into `Styles::classes`, but the renderer
//! does not draw them.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use cm_core::text::Text;
use cm_core::util::misc::count_column;
use cm_core::util::string_stream::{LineOracle, StringStream};
use cm_core::{Doc, LineHandle, Pos};
use cm_modes::{
    ConfigValue, JsArg, Mode, State, copy_state, read_token, start_state, with_inner_mode_mut,
};

/// `SavedContext` / a bare saved state: what `line.stateAfter` holds.
#[derive(Debug)]
pub struct SavedContext {
    pub state: State,
    /// `lookAhead`; 0 stands for "saved as a plain state".
    pub look_ahead: i64,
}

/// `line.styles`: `[modeGen, end, style, end, style, ...]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Styles {
    pub mode_gen: u64,
    /// `(end, style)` pairs.
    pub spans: Vec<(usize, Option<Rc<str>>)>,
    /// `line.styleClasses`: `(bgClass, textClass)`.
    pub classes: Option<(Option<String>, Option<String>)>,
}

/// The line-oracle part of a `Context`: `lookAhead` reads further lines.
pub struct CtxOracle<'a> {
    pub doc: &'a Doc,
    pub line: i64,
    pub max_look_ahead: Cell<i64>,
}

impl LineOracle for CtxOracle<'_> {
    /// `Context.prototype.lookAhead(n)`.
    fn look_ahead(&self, n: i64) -> Option<Text> {
        let line = self.doc.get_line_utf16(self.line + n);
        if line.is_some() && n > self.max_look_ahead.get() {
            self.max_look_ahead.set(n);
        }
        line
    }
    /// `baseToken`: only overlays set `baseTokens` (overlays are not ported).
    fn base_token(&self, _pos: usize) -> Option<(Option<String>, i64)> {
        None
    }
}

/// `Context`: a mode state plus the line it applies to.
pub struct Context {
    pub state: State,
    pub line: i64,
    pub max_look_ahead: i64,
}

impl Context {
    /// `nextLine()`.
    pub fn next_line(&mut self) {
        self.line += 1;
        if self.max_look_ahead > 0 {
            self.max_look_ahead -= 1;
        }
    }
    /// `save(copy)`.
    pub fn save(&self, mode: &dyn Mode, copy: bool) -> Rc<dyn Any> {
        let state = if copy {
            copy_state(mode, &self.state)
        } else {
            self.state.clone()
        };
        Rc::new(SavedContext {
            state,
            look_ahead: self.max_look_ahead,
        })
    }
    /// `Context.fromSaved(doc, saved, line)`.
    pub fn from_saved(mode: &dyn Mode, saved: &SavedContext, line: i64) -> Context {
        Context {
            state: copy_state(mode, &saved.state),
            line,
            max_look_ahead: saved.look_ahead,
        }
    }
}

/// The editor-side highlighting bookkeeping (`doc.modeFrontier`,
/// `doc.highlightFrontier`, `cm.state.modeGen`) and options it reads.
#[derive(Clone, Debug)]
pub struct HighlightState {
    pub mode_frontier: i64,
    pub highlight_frontier: i64,
    pub mode_gen: u64,
    pub tab_size: i64,
    pub max_highlight_length: usize,
    pub flatten_spans: bool,
    /// `options.addModeClass`: prefix each style with `m-<inner mode name>`.
    pub add_mode_class: bool,
    /// `display.viewFrom` / `display.viewTo`: lines in view get their state saved.
    pub view_from: i64,
    pub view_to: i64,
}

impl HighlightState {
    pub fn new(first: i64) -> HighlightState {
        HighlightState {
            mode_frontier: first,
            highlight_frontier: first,
            mode_gen: 1,
            tab_size: 4,
            max_highlight_length: 10000,
            flatten_spans: true,
            add_mode_class: false,
            view_from: first,
            view_to: first,
        }
    }
}

/// The JS `mode.startState` presence test (`if (!doc.mode.startState)`).
fn has_start_state(mode: &dyn Mode) -> bool {
    mode.start_state(JsArg::Undefined, JsArg::Undefined)
        .is_some()
}

fn saved_of(h: &LineHandle) -> Option<Rc<dyn Any>> {
    h.state_after()
}
fn as_saved(v: &Rc<dyn Any>) -> &SavedContext {
    v.downcast_ref::<SavedContext>()
        .expect("stateAfter holds a SavedContext")
}

/// `callBlankLine(mode, state)`: the mode's `blankLine`, else the innermost mode's,
/// on the inner state itself (as the JS `innerMode` result is).
fn call_blank_line(mode: &Rc<dyn Mode>, state: &mut State) -> Option<String> {
    if mode.has_blank_line() {
        return mode.blank_line(state);
    }
    with_inner_mode_mut(mode, state, |inner, s| {
        if inner.has_blank_line() {
            inner.blank_line(s)
        } else {
            None
        }
    })
}

/// `processLine(cm, text, context, startAt)`.
pub fn process_line(
    doc: &Doc,
    mode: &Rc<dyn Mode>,
    hs: &HighlightState,
    text: &[u16],
    ctx: &mut Context,
    start_at: usize,
) {
    let oracle = CtxOracle {
        doc,
        line: ctx.line,
        max_look_ahead: Cell::new(ctx.max_look_ahead),
    };
    {
        let mut stream = StringStream::new(text, Some(hs.tab_size), Some(&oracle));
        stream.start = start_at;
        stream.pos = start_at;
        if text.is_empty() {
            call_blank_line(mode, &mut ctx.state);
        }
        while !stream.eol() {
            if let Err(e) = read_token(mode, &mut stream, &mut ctx.state, None) {
                panic!("{e}");
            }
            stream.start = stream.pos;
        }
    }
    ctx.max_look_ahead = oracle.max_look_ahead.get();
}

/// `findStartLine(cm, n, precise)`.
fn find_start_line(
    doc: &Doc,
    mode: &Rc<dyn Mode>,
    hs: &HighlightState,
    n: i64,
    precise: bool,
) -> i64 {
    let first = doc.first_line();
    let has_inner = mode_has_inner(mode);
    let lim = if precise {
        -1
    } else {
        n - if has_inner { 1000 } else { 100 }
    };
    let mut minindent: Option<i64> = None;
    let mut minline: i64 = 0;
    let mut search = n;
    while search > lim {
        if search <= first {
            return first;
        }
        let line = doc.get_line_handle(search - 1).expect("line in range");
        if let Some(after) = saved_of(&line) {
            let la = as_saved(&after).look_ahead;
            if !precise || search + la <= hs.mode_frontier {
                return search;
            }
        }
        let text = line.text_utf16();
        let indented = count_column(&text, None, hs.tab_size, 0, 0);
        if minindent.is_none() || minindent.unwrap() > indented {
            minline = search - 1;
            minindent = Some(indented);
        }
        search -= 1;
    }
    minline
}

/// Whether the mode object has an `innerMode` method ([`Mode::has_inner_mode`]).
pub fn mode_has_inner(mode: &Rc<dyn Mode>) -> bool {
    mode.has_inner_mode()
}

/// `getContextBefore(cm, n, precise)`.
pub fn get_context_before(
    doc: &Doc,
    mode: &Rc<dyn Mode>,
    hs: &mut HighlightState,
    n: i64,
    precise: bool,
) -> Context {
    if !has_start_state(&**mode) {
        return Context {
            state: State::True,
            line: n,
            max_look_ahead: 0,
        };
    }
    let first = doc.first_line();
    let start = find_start_line(doc, mode, hs, n, precise);
    let saved = if start > first {
        doc.get_line_handle(start - 1).and_then(|l| saved_of(&l))
    } else {
        None
    };
    let mut ctx = match saved {
        Some(s) => Context::from_saved(&**mode, as_saved(&s), start),
        None => Context {
            state: start_state(&**mode, JsArg::Undefined, JsArg::Undefined),
            line: start,
            max_look_ahead: 0,
        },
    };
    for ln in start..n {
        let line = doc.get_line_handle(ln).expect("line in range");
        let text = line.text_utf16();
        process_line(doc, mode, hs, &text, &mut ctx, 0);
        let pos = ctx.line;
        let keep = pos == n - 1 || pos % 5 == 0 || (pos >= hs.view_from && pos < hs.view_to);
        line.set_state_after(if keep {
            Some(ctx.save(&**mode, true))
        } else {
            None
        });
        ctx.next_line();
    }
    if precise {
        hs.mode_frontier = ctx.line;
    }
    ctx
}

/// `extractLineClasses(type, output)`.
fn extract_line_classes(
    ty: Option<String>,
    output: &mut (Option<Vec<String>>, Option<Vec<String>>),
) -> Option<String> {
    let mut ty = ty?;
    loop {
        // /(?:^|\s+)line-(background-)?(\S+)/
        let bytes = ty.as_bytes();
        let mut found = None;
        let mut i = 0;
        while i < bytes.len() {
            let at_start = i == 0 || bytes[i - 1].is_ascii_whitespace();
            if at_start && ty[i..].starts_with("line-") {
                // Include the preceding whitespace run in the match.
                let mut ws = i;
                while ws > 0 && bytes[ws - 1].is_ascii_whitespace() {
                    ws -= 1;
                }
                let rest = &ty[i + 5..];
                let (bg, rest2, off) = if let Some(r) = rest.strip_prefix("background-") {
                    (true, r, i + 5 + 11)
                } else {
                    (false, rest, i + 5)
                };
                let cls_len = rest2
                    .find(|c: char| c.is_whitespace())
                    .unwrap_or(rest2.len());
                if cls_len > 0 {
                    found = Some((ws, off + cls_len, bg, rest2[..cls_len].to_string()));
                    break;
                }
            }
            i += 1;
        }
        let Some((s, e, bg, cls)) = found else { break };
        ty = format!("{}{}", &ty[..s], &ty[e..]);
        let slot = if bg { &mut output.0 } else { &mut output.1 };
        match slot {
            None => *slot = Some(vec![cls]),
            Some(cur) => {
                if !cur.contains(&cls) {
                    cur.push(cls);
                }
            }
        }
    }
    Some(ty)
}

/// `runMode(cm, text, mode, context, f, lineClasses, forceToEnd)` from highlight.js.
fn run_mode_line(
    doc: &Doc,
    mode: &Rc<dyn Mode>,
    hs: &HighlightState,
    text: &[u16],
    ctx: &mut Context,
    f: &mut dyn FnMut(usize, Option<String>),
    line_classes: &mut (Option<Vec<String>>, Option<Vec<String>>),
    force_to_end: bool,
) {
    // `mode.flattenSpans`, unless null/undefined, beats `cm.options.flattenSpans`.
    let mut flatten = match mode.prop("flattenSpans") {
        None | Some(ConfigValue::Null) => hs.flatten_spans,
        Some(v) => v.truthy(),
    };
    // `cm.options.addModeClass && [null]`: `readToken` stores the inner mode here.
    let mut inner: Option<Option<Rc<dyn Mode>>> = hs.add_mode_class.then_some(None);
    let mut cur_start = 0usize;
    let mut cur_style: Option<String> = None;
    let oracle = CtxOracle {
        doc,
        line: ctx.line,
        max_look_ahead: Cell::new(ctx.max_look_ahead),
    };
    let mut need_process_rest: Option<usize> = None;
    let final_pos;
    {
        let mut stream = StringStream::new(text, Some(hs.tab_size), Some(&oracle));
        if text.is_empty() {
            // `extractLineClasses(callBlankLine(mode, context.state), lineClasses)`.
            extract_line_classes(call_blank_line(mode, &mut ctx.state), line_classes);
        }
        while !stream.eol() {
            let mut style;
            if stream.pos > hs.max_highlight_length {
                flatten = false;
                if force_to_end {
                    need_process_rest = Some(stream.pos);
                }
                stream.pos = text.len();
                style = None;
            } else {
                let tok = read_token(mode, &mut stream, &mut ctx.state, inner.as_mut())
                    .unwrap_or_else(|e| panic!("{e}"));
                style = extract_line_classes(tok, line_classes);
            }
            // `style = "m-" + (style ? mName + " " + style : mName)` for a named
            // inner mode.
            if let Some(Some(im)) = &inner {
                let m_name = im.name();
                if !m_name.is_empty() {
                    style = Some(match style {
                        Some(s) if !s.is_empty() => format!("m-{m_name} {s}"),
                        _ => format!("m-{m_name}"),
                    });
                }
            }
            if !flatten || cur_style != style {
                while cur_start < stream.start {
                    cur_start = stream.start.min(cur_start + 5000);
                    f(cur_start, cur_style.clone());
                }
                cur_style = style;
            }
            stream.start = stream.pos;
            if need_process_rest.is_some() {
                break;
            }
        }
        final_pos = stream.pos;
    }
    ctx.max_look_ahead = oracle.max_look_ahead.get();
    if let Some(p) = need_process_rest {
        // `if (forceToEnd) processLine(cm, text, context, stream.pos)` runs before
        // the position jumps to the end.
        process_line(doc, mode, hs, text, ctx, p);
    }
    while cur_start < final_pos {
        let pos = final_pos.min(cur_start + 5000);
        f(pos, cur_style.clone());
        cur_start = pos;
    }
}

/// `highlightLine(cm, line, context, forceToEnd)` (no overlays).
pub fn highlight_line(
    doc: &Doc,
    mode: &Rc<dyn Mode>,
    hs: &HighlightState,
    text: &[u16],
    ctx: &mut Context,
    force_to_end: bool,
) -> Styles {
    let mut spans: Vec<(usize, Option<Rc<str>>)> = Vec::new();
    let mut classes_vec = (None, None);
    run_mode_line(
        doc,
        mode,
        hs,
        text,
        ctx,
        &mut |end, style| spans.push((end, style.map(|s| Rc::from(s.as_str())))),
        &mut classes_vec,
        force_to_end,
    );
    let classes = (
        classes_vec.0.map(|v| v.join(" ")),
        classes_vec.1.map(|v| v.join(" ")),
    );
    let classes = if classes.0.is_some() || classes.1.is_some() {
        Some(classes)
    } else {
        None
    };
    Styles {
        mode_gen: hs.mode_gen,
        spans,
        classes,
    }
}

/// `getLineStyles(cm, line, updateFrontier)`.
pub fn get_line_styles(
    doc: &Doc,
    mode: &Rc<dyn Mode>,
    hs: &mut HighlightState,
    line_no: i64,
    update_frontier: Option<i64>,
) -> Rc<Styles> {
    let line = doc.get_line_handle(line_no).expect("line in range");
    if let Some(s) = line.styles() {
        if let Ok(st) = s.downcast::<Styles>() {
            if st.mode_gen == hs.mode_gen {
                return st;
            }
        }
    }
    let mut ctx = get_context_before(doc, mode, hs, line_no, false);
    let text = line.text_utf16();
    let reset_state = if text.len() > hs.max_highlight_length {
        Some(copy_state(&**mode, &ctx.state))
    } else {
        None
    };
    let result = highlight_line(doc, mode, hs, &text, &mut ctx, false);
    let copy = reset_state.is_none();
    if let Some(r) = reset_state {
        ctx.state = r;
    }
    line.set_state_after(Some(ctx.save(&**mode, copy)));
    let rc = Rc::new(result);
    line.set_styles(Some(rc.clone() as Rc<dyn Any>));
    if update_frontier == Some(hs.highlight_frontier) {
        hs.highlight_frontier += 1;
        hs.mode_frontier = hs.mode_frontier.max(hs.highlight_frontier);
    }
    rc
}

/// `retreatFrontier(doc, n)`.
pub fn retreat_frontier(doc: &Doc, hs: &mut HighlightState, n: i64) {
    hs.mode_frontier = hs.mode_frontier.min(n);
    if hs.highlight_frontier < n - 10 {
        return;
    }
    let mut start = doc.first_line();
    let mut line = n - 1;
    while line > doc.first_line() {
        if let Some(saved) = doc.get_line_handle(line).and_then(|h| saved_of(&h)) {
            let la = as_saved(&saved).look_ahead;
            if la == 0 || line + la < n {
                start = line + 1;
                break;
            }
        }
        line -= 1;
    }
    hs.highlight_frontier = hs.highlight_frontier.min(start);
}

/// `resetModeState(cm)`: drop every cached state and style.
pub fn reset_mode_state(doc: &Doc, hs: &mut HighlightState) {
    let first = doc.first_line();
    doc.each_line(first, first + doc.line_count() as i64, |l| {
        l.set_state_after(None);
        l.set_styles(None);
        false
    });
    hs.mode_frontier = first;
    hs.highlight_frontier = first;
    hs.mode_gen += 1;
}

/// `highlightWorker(cm)` with a line budget instead of a time budget. Returns the
/// lines whose styles changed, and whether more work remains.
pub fn highlight_worker(
    doc: &Doc,
    mode: &Rc<dyn Mode>,
    hs: &mut HighlightState,
    max_lines: usize,
) -> (Vec<i64>, bool) {
    let mut changed = Vec::new();
    if hs.highlight_frontier >= hs.view_to {
        return (changed, false);
    }
    let mut ctx = get_context_before(doc, mode, hs, hs.highlight_frontier, false);
    let end = (doc.first_line() + doc.line_count() as i64).min(hs.view_to + 500);
    let mut done = 0usize;
    let mut more = false;
    while ctx.line < end {
        let line = doc.get_line_handle(ctx.line).expect("line in range");
        let text = line.text_utf16();
        if ctx.line >= hs.view_from {
            let old = line.styles().and_then(|s| s.downcast::<Styles>().ok());
            let reset_state = if text.len() > hs.max_highlight_length {
                Some(copy_state(&**mode, &ctx.state))
            } else {
                None
            };
            let st = highlight_line(doc, mode, hs, &text, &mut ctx, true);
            if let Some(r) = reset_state {
                ctx.state = r;
            }
            let is_change = match &old {
                None => true,
                Some(o) => {
                    o.spans != st.spans || o.classes != st.classes || o.mode_gen != st.mode_gen
                }
            };
            if is_change {
                changed.push(ctx.line);
            }
            line.set_styles(Some(Rc::new(st) as Rc<dyn Any>));
            line.set_state_after(Some(ctx.save(&**mode, true)));
        } else {
            if text.len() <= hs.max_highlight_length {
                process_line(doc, mode, hs, &text, &mut ctx, 0);
            }
            line.set_state_after(if ctx.line % 5 == 0 {
                Some(ctx.save(&**mode, true))
            } else {
                None
            });
        }
        ctx.next_line();
        done += 1;
        if done >= max_lines {
            more = ctx.line < end;
            break;
        }
    }
    hs.highlight_frontier = ctx.line;
    hs.mode_frontier = hs.mode_frontier.max(ctx.line);
    (changed, more)
}

/// A `getTokenAt` result (`Token`).
#[derive(Debug, Clone)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub string: String,
    pub ty: Option<String>,
    pub state: State,
}

/// `takeToken(cm, pos, precise, asArray)`.
pub fn take_token(
    doc: &Doc,
    mode: &Rc<dyn Mode>,
    hs: &mut HighlightState,
    pos: Pos,
    precise: bool,
    as_array: bool,
) -> (Token, Vec<Token>) {
    let pos = doc.clip_pos(pos);
    let line = doc.get_line_handle(pos.line).expect("line in range");
    let mut ctx = get_context_before(doc, mode, hs, pos.line, precise);
    let text = line.text_utf16();
    let oracle = CtxOracle {
        doc,
        line: ctx.line,
        max_look_ahead: Cell::new(ctx.max_look_ahead),
    };
    let mut stream = StringStream::new(&text, Some(hs.tab_size), Some(&oracle));
    let mut tokens = Vec::new();
    let mut style = None;
    while (as_array || (stream.pos as i64) < pos.ch) && !stream.eol() {
        stream.start = stream.pos;
        style =
            read_token(mode, &mut stream, &mut ctx.state, None).unwrap_or_else(|e| panic!("{e}"));
        if as_array {
            tokens.push(Token {
                start: stream.start,
                end: stream.pos,
                string: stream.current_string(),
                ty: style.clone(),
                state: copy_state(&**mode, &ctx.state),
            });
        }
    }
    let tok = Token {
        start: stream.start,
        end: stream.pos,
        string: stream.current_string(),
        ty: style,
        state: ctx.state.clone(),
    };
    (tok, tokens)
}
