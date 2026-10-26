//! Port of `addon/mode/overlay.js` (CodeMirror 5.65.21): `CodeMirror.overlayMode`.
//!
//! Runs a base mode and an overlay mode over the same text and merges their styles.
//! Each mode advances on its own; a token ends where the nearer of the two token
//! ends is. The overlay style replaces the base style unless `combine` is set (or
//! the overlay state's `combineTokens` is true), in which case both are joined.
//!
//! JS quirks kept: with `combine` and no `combineTokens`, a null base style is
//! joined as the string `"null"` (`state.baseCur + " " + state.overlayCur`).
//! `stream != state.streamSeen` compares stream identity ([`StringStream::id`]).

use std::rc::Rc;

use cm_core::util::string_stream::StringStream;

use crate::{JsArg, Mode, State, copy_state, start_state};

/// `CodeMirror.overlayMode(base, overlay, combine)`.
pub struct OverlayMode {
    base: Rc<dyn Mode>,
    overlay: Rc<dyn Mode>,
    combine: bool,
}

/// The overlay mode's state.
#[derive(Clone, Debug)]
pub struct OverlayState {
    pub base: State,
    pub overlay: State,
    pub base_pos: usize,
    pub base_cur: Option<String>,
    pub overlay_pos: usize,
    pub overlay_cur: Option<String>,
    /// `streamSeen`: the id of the last stream; `None` after `copyState`.
    pub stream_seen: Option<u64>,
}

/// `CodeMirror.overlayMode(base, overlay, combine)`. The result has no name until
/// a `defineMode` factory returns it through `getMode`.
pub fn overlay_mode(base: Rc<dyn Mode>, overlay: Rc<dyn Mode>, combine: bool) -> Rc<dyn Mode> {
    Rc::new(OverlayMode {
        base,
        overlay,
        combine,
    })
}

impl Mode for OverlayMode {
    fn name(&self) -> &str {
        ""
    }
    fn start_state(&self, _a1: JsArg, _a2: JsArg) -> Option<State> {
        Some(State::new(OverlayState {
            base: start_state(&*self.base, JsArg::Undefined, JsArg::Undefined),
            overlay: start_state(&*self.overlay, JsArg::Undefined, JsArg::Undefined),
            base_pos: 0,
            base_cur: None,
            overlay_pos: 0,
            overlay_cur: None,
            stream_seen: None,
        }))
    }
    fn copy_state(&self, state: &State) -> Option<State> {
        let s = state.get::<OverlayState>();
        Some(State::new(OverlayState {
            base: copy_state(&*self.base, &s.base),
            overlay: copy_state(&*self.overlay, &s.overlay),
            base_pos: s.base_pos,
            base_cur: None,
            overlay_pos: s.overlay_pos,
            overlay_cur: None,
            stream_seen: None,
        }))
    }
    fn token(&self, stream: &mut StringStream<'_>, state: &mut State) -> Option<String> {
        let s = state.get_mut::<OverlayState>();
        if s.stream_seen != Some(stream.id()) || s.base_pos.min(s.overlay_pos) < stream.start {
            s.stream_seen = Some(stream.id());
            s.base_pos = stream.start;
            s.overlay_pos = stream.start;
        }
        if stream.start == s.base_pos {
            s.base_cur = self.base.token(stream, &mut s.base);
            s.base_pos = stream.pos;
        }
        if stream.start == s.overlay_pos {
            stream.pos = stream.start;
            s.overlay_cur = self.overlay.token(stream, &mut s.overlay);
            s.overlay_pos = stream.pos;
        }
        stream.pos = s.base_pos.min(s.overlay_pos);

        // state.overlay.combineTokens always takes precedence over combine,
        // unless set to null
        let combine_tokens = self.overlay.combine_tokens(&s.overlay);
        let Some(over) = &s.overlay_cur else {
            return s.base_cur.clone();
        };
        if (s.base_cur.is_some() && combine_tokens == Some(true))
            || (self.combine && combine_tokens.is_none())
        {
            Some(format!(
                "{} {}",
                s.base_cur.as_deref().unwrap_or("null"),
                over
            ))
        } else {
            Some(over.clone())
        }
    }
    fn has_indent(&self) -> bool {
        self.base.has_indent()
    }
    fn indent(&self, state: &State, text_after: &[u16], line: &[u16]) -> Option<f64> {
        if !self.base.has_indent() {
            return None;
        }
        self.base
            .indent(&state.get::<OverlayState>().base, text_after, line)
    }
    fn electric_chars(&self) -> Option<&str> {
        self.base.electric_chars()
    }
    fn has_inner_mode(&self) -> bool {
        true
    }
    fn inner_mode(&self, state: &State) -> Option<(Rc<dyn Mode>, State)> {
        Some((self.base.clone(), state.get::<OverlayState>().base.clone()))
    }
    fn inner_mode_mut(
        &self,
        state: &mut State,
        f: &mut dyn FnMut(&Rc<dyn Mode>, &mut State),
    ) -> bool {
        f(&self.base, &mut state.get_mut::<OverlayState>().base);
        true
    }
    fn has_blank_line(&self) -> bool {
        true
    }
    fn blank_line(&self, state: &mut State) -> Option<String> {
        let s = state.get_mut::<OverlayState>();
        let base_token = if self.base.has_blank_line() {
            self.base.blank_line(&mut s.base)
        } else {
            None
        };
        let overlay_token = if self.overlay.has_blank_line() {
            self.overlay.blank_line(&mut s.overlay)
        } else {
            None
        };
        match overlay_token {
            None => base_token,
            Some(o) => match base_token {
                Some(b) if self.combine => Some(format!("{b} {o}")),
                _ => Some(o),
            },
        }
    }
}
