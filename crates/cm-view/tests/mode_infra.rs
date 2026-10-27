//! The editor side of the P10-00 mode infrastructure (`src/line/highlight.js`,
//! `src/edit/methods.js`): `callBlankLine` reaches an inner mode's own state,
//! `blankLine` results become line classes, `getModeAt` sees through composite
//! modes, and `getHelpers` looks helpers up for the mode at a position.

use std::rc::Rc;

use cm_core::Pos;
use cm_core::util::string_stream::StringStream;
use cm_modes::overlay::overlay_mode;
use cm_modes::simple::{
    LocalModeSpec, RegexSpec, Rule, SimpleStates, TokenSpec, define_simple_mode,
};
use cm_modes::{JsArg, Mode, Registry, Spec, State};
use cm_view::{Editor, EditorOptions, Metrics};

/// Counts the blank lines it is told about and styles each token with the count.
struct Counting;

impl Mode for Counting {
    fn name(&self) -> &str {
        "counting"
    }
    fn start_state(&self, _: JsArg, _: JsArg) -> Option<State> {
        Some(State::new(0u32))
    }
    fn token(&self, stream: &mut StringStream<'_>, state: &mut State) -> Option<String> {
        stream.skip_to_end();
        Some(format!("n{}", state.get::<u32>()))
    }
    fn has_blank_line(&self) -> bool {
        true
    }
    fn blank_line(&self, state: &mut State) -> Option<String> {
        *state.get_mut::<u32>() += 1;
        None
    }
}

/// `<<` enters `counting` until `>>`.
fn registry() -> Registry {
    let mut r = Registry::with_all_modes();
    r.define_mode(
        "counting",
        Rc::new(|_, _, _| Rc::new(Counting) as Rc<dyn Mode>),
    );
    define_simple_mode(
        &mut r,
        "embed",
        SimpleStates {
            states: vec![(
                "start".into(),
                vec![Rule {
                    mode: Some(LocalModeSpec {
                        spec: Some(Spec::from("counting")),
                        end: RegexSpec::Str(">>".into()),
                        ..Default::default()
                    }),
                    ..Rule::new(RegexSpec::Str("<<".into()), TokenSpec::Str("meta".into()))
                }],
            )],
            meta: None,
        },
    )
    .unwrap();
    r
}

fn editor(text: &str, mode: &str, r: Registry) -> Editor {
    Editor::with_registry(
        text,
        Spec::from(mode),
        EditorOptions::default(),
        Metrics::test_grid(),
        Rc::new(r),
    )
}

/// CM5 `callBlankLine` runs the inner mode's `blankLine` on the inner state itself,
/// so the count reaches the next line (`n2`, not `n0`).
#[test]
fn blank_line_reaches_the_inner_state() {
    let ed = editor("<<\n\n\nx\n>>", "embed", registry());
    assert_eq!(ed.get_token_type_at(Pos::new(3, 1)).as_deref(), Some("n2"));
    let styles = ed.line_styles(3);
    assert_eq!(styles.spans.last().unwrap().1.as_deref(), Some("n2"));
}

#[test]
fn get_mode_at_sees_the_local_mode() {
    let ed = editor("ab <<\nx\n>> c", "embed", registry());
    assert_eq!(ed.get_mode_at(Pos::new(0, 1)).name(), "embed");
    assert_eq!(ed.get_mode_at(Pos::new(1, 1)).name(), "counting");
    assert_eq!(ed.get_mode_at(Pos::new(2, 4)).name(), "embed");
}

#[test]
fn helpers_follow_the_mode_at_the_position() {
    let mut r = registry();
    r.helpers.register("hint", "counting", Rc::new("inner"));
    r.helpers.register("hint", "embed", Rc::new("outer"));
    let ed = editor("ab <<\nx\n>> c", "embed", r);
    let at = |line, ch| {
        ed.get_helper(Pos::new(line, ch), "hint")
            .map(|h| *h.downcast_ref::<&str>().unwrap())
    };
    assert_eq!(at(0, 1), Some("outer"));
    assert_eq!(at(1, 1), Some("inner"));
    assert!(ed.get_helpers(Pos::new(1, 1), "nope").is_empty());
}

/// An overlay's `blankLine` result is scanned for `line-` classes.
struct BlankClass;

impl Mode for BlankClass {
    fn name(&self) -> &str {
        ""
    }
    fn token(&self, stream: &mut StringStream<'_>, _: &mut State) -> Option<String> {
        stream.skip_to_end();
        None
    }
    fn has_blank_line(&self) -> bool {
        true
    }
    fn blank_line(&self, _: &mut State) -> Option<String> {
        Some("line-blank line-background-empty".into())
    }
}

#[test]
fn blank_line_result_becomes_line_classes() {
    let mut r = Registry::with_all_modes();
    r.define_mode(
        "blankclass",
        Rc::new(|reg, opts, _| {
            overlay_mode(
                reg.get_mode(opts, &Spec::from("text/plain")),
                Rc::new(BlankClass),
                false,
            )
        }),
    );
    let ed = editor("a\n\nb", "blankclass", r);
    assert_eq!(
        ed.line_styles(1).classes,
        Some((Some("empty".into()), Some("blank".into())))
    );
    assert_eq!(ed.line_styles(0).classes, None);
}
