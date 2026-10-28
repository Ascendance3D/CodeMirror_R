//! `addLineClass`/`removeLineClass` (src/model/Doc.js:265-291) and `changeLine`
//! (src/model/changes.js:332), after the text, background and wrap parts of CM5's
//! `addLineClass` test (test/test.js:1756). The gutter part needs gutters (#36); its
//! property is stored here but not rendered.

use cm_core::{Doc, DocOptions, EditorSemantics, LineWhere};

fn doc(value: &str) -> Doc {
    let _clock = cm_core::clock::manual(1_000_000.0);
    let d = Doc::with_options(DocOptions {
        text: value.to_string(),
        ..Default::default()
    });
    d.attach_editor_semantics(EditorSemantics::default());
    d
}

#[test]
fn add_text_background_and_wrap() {
    let d = doc("hohoho\nsecond");
    d.add_line_class(0, LineWhere::Text, "foo");
    d.add_line_class(0, LineWhere::Text, "bar");
    d.add_line_class(0, LineWhere::Background, "baz");
    d.add_line_class(0, LineWhere::Wrap, "foo");
    let h = d.get_line_handle(0).unwrap();
    assert_eq!(h.text_class().as_deref(), Some("foo bar"));
    assert_eq!(h.bg_class().as_deref(), Some("baz"));
    assert_eq!(h.wrap_class().as_deref(), Some("foo"));
    assert_eq!(h.gutter_class(), None);
    // Line 1 is untouched.
    assert_eq!(d.get_line_handle(1).unwrap().text_class(), None);
}

#[test]
fn add_is_idempotent_and_matches_whole_words() {
    let d = doc("x");
    d.add_line_class(0, LineWhere::Text, "foo");
    d.add_line_class(0, LineWhere::Text, "foo");
    d.add_line_class(0, LineWhere::Text, "fo");
    d.add_line_class(0, LineWhere::Text, "oo");
    let h = d.get_line_handle(0).unwrap();
    assert_eq!(h.text_class().as_deref(), Some("foo fo oo"));
}

#[test]
fn unknown_where_is_wrap() {
    let d = doc("x");
    d.add_line_class(0, LineWhere::parse("anything"), "w");
    d.add_line_class(0, LineWhere::parse("gutter"), "g");
    let h = d.get_line_handle(0).unwrap();
    assert_eq!(h.wrap_class().as_deref(), Some("w"));
    assert_eq!(h.gutter_class().as_deref(), Some("g"));
}

#[test]
fn remove_splices_and_clears() {
    let d = doc("x");
    for c in ["a", "b", "c"] {
        d.add_line_class(0, LineWhere::Text, c);
    }
    let h = d.get_line_handle(0).unwrap();
    d.remove_line_class(0, LineWhere::Text, Some("b"));
    assert_eq!(h.text_class().as_deref(), Some("a c"));
    d.remove_line_class(0, LineWhere::Text, Some("a"));
    assert_eq!(h.text_class().as_deref(), Some("c"));
    // A class that is not there: no change.
    d.remove_line_class(0, LineWhere::Text, Some("zzz"));
    assert_eq!(h.text_class().as_deref(), Some("c"));
    d.remove_line_class(0, LineWhere::Text, Some("c"));
    assert_eq!(h.text_class(), None);
    // No class argument clears the property.
    d.add_line_class(0, LineWhere::Wrap, "p");
    d.add_line_class(0, LineWhere::Wrap, "q");
    d.remove_line_class(0, LineWhere::Wrap, None);
    assert_eq!(h.wrap_class(), None);
}

#[test]
fn remove_last_class_keeps_leading_ones() {
    let d = doc("x");
    d.add_line_class(0, LineWhere::Text, "a");
    d.add_line_class(0, LineWhere::Text, "b");
    d.remove_line_class(0, LineWhere::Text, Some("b"));
    assert_eq!(
        d.get_line_handle(0).unwrap().text_class().as_deref(),
        Some("a")
    );
}

#[test]
fn numbers_clip_and_handles_follow_the_line() {
    let d = doc("a\nb\nc");
    // `changeLine` clips a number into the document.
    let h = d.add_line_class(99, LineWhere::Text, "last").unwrap();
    assert_eq!(d.get_line_number(&h), Some(2));
    let first = d.add_line_class(-5, LineWhere::Text, "first").unwrap();
    assert_eq!(d.get_line_number(&first), Some(0));
    // A handle keeps its line when lines are inserted above it.
    d.replace_range("new\n", cm_core::Pos::new(0, 0), None, None);
    assert_eq!(d.get_line_number(&h), Some(3));
    d.add_line_class(&h, LineWhere::Background, "bg");
    assert_eq!(
        d.get_line_handle(3).unwrap().bg_class().as_deref(),
        Some("bg")
    );
}

#[test]
fn removed_line_handle_returns_none() {
    let d = doc("a\nb");
    let h = d.get_line_handle(1).unwrap();
    d.set_value("only");
    assert!(d.add_line_class(&h, LineWhere::Text, "x").is_none());
    assert!(d.remove_line_class(&h, LineWhere::Text, None).is_none());
}

/// testCM("setValueClears") line class half: `setValue` replaces the lines.
#[test]
fn set_value_clears_line_classes() {
    let d = doc("a\nb");
    d.add_line_class(0, LineWhere::Wrap, "foo");
    d.set_value("foo");
    assert_eq!(d.get_line_handle(0).unwrap().wrap_class(), None);
}
