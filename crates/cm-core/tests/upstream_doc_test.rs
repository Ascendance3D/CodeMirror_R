//! Port of CodeMirror 5 `test/doc_test.js` (model-level). Every test is named
//! `doc_<name>` after the JS `testDoc("<name>", ...)` it reproduces (the JS
//! registers them as "doc_<name>" too).
//!
//! Editors (`CodeMirror(place, ...)`) are documents with
//! `attach_editor_semantics(EditorSemantics::default())`; `Doc`s (`A*`) are bare.
//!
//! Not ported (view or mode features, no model behavior): `swapDoc`,
//! `docKeepsScroll`, `docKeepsMode`. `copyDoc` uses `swapDoc` only to route calls to
//! the copy, so it is ported calling the copy directly.

use cm_core::{Doc, EditorSemantics, LinkedDocOptions, MarkOptions, Marker, MarkerFind, Pos};
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

fn p(line: i64, ch: i64) -> Pos {
    Pos::new(line, ch)
}
fn pl(line: i64) -> Pos {
    Pos::line_end(line)
}

fn editor(doc: Doc) -> Doc {
    doc.attach_editor_semantics(EditorSemantics::default());
    doc
}

/// `instantiateSpec(spec)`: the JS minilanguage
/// `/^(\w+)(\*?)(?:='([^\']*)'|<(~?)(\w+)(?:\/(\d+)-(\d+))?)\s*/`.
fn spec(mut s: &str) -> Vec<Doc> {
    let mut names: HashMap<String, usize> = HashMap::new();
    let mut editors: Vec<Doc> = Vec::new();
    while !s.is_empty() {
        let name_len = s
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(s.len());
        let name = s[..name_len].to_string();
        s = &s[name_len..];
        let is_doc = s.starts_with('*');
        if is_doc {
            s = &s[1..];
        }
        let cur = if let Some(rest) = s.strip_prefix("='") {
            let end = rest.find('\'').unwrap();
            let text = &rest[..end];
            s = &rest[end + 1..];
            let d = Doc::new(text);
            if is_doc { d } else { editor(d) }
        } else {
            let rest = s.strip_prefix('<').expect("spec");
            let (sep, rest) = match rest.strip_prefix('~') {
                Some(r) => (true, r),
                None => (false, rest),
            };
            let olen = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            let other = rest[..olen].to_string();
            let mut rest = &rest[olen..];
            let (mut from, mut to) = (None, None);
            if let Some(r) = rest.strip_prefix('/') {
                let dash = r.find('-').unwrap();
                let tend = r[dash + 1..]
                    .find(|c: char| !c.is_ascii_digit())
                    .map(|i| dash + 1 + i)
                    .unwrap_or(r.len());
                from = Some(r[..dash].parse().unwrap());
                to = Some(r[dash + 1..tend].parse().unwrap());
                rest = &r[tend..];
            }
            s = rest;
            if !names.contains_key(&other) {
                names.insert(other.clone(), editors.len());
                editors.push(editor(Doc::new("")));
            }
            let d = editors[names[&other]]
                .linked_doc(LinkedDocOptions {
                    shared_hist: !sep,
                    from,
                    to,
                    mode: None,
                })
                .unwrap();
            if is_doc { d } else { editor(d) }
        };
        names.insert(name, editors.len());
        editors.push(cur);
        s = s.trim_start();
    }
    editors
}

fn eq_all(val: &str, docs: &[&Doc]) {
    for (i, d) in docs.iter().enumerate() {
        assert_eq!(d.get_value(None), val, "editor #{i}");
    }
}

fn from_of(m: &Marker) -> Pos {
    match m.find() {
        Some(MarkerFind::Range { from, .. }) => from,
        other => panic!("expected a range, got {other:?}"),
    }
}
fn to_of(m: &Marker) -> Pos {
    match m.find() {
        Some(MarkerFind::Range { to: Some(to), .. }) => to,
        other => panic!("expected a range, got {other:?}"),
    }
}
fn eq_char_pos(a: Pos, b: Pos) {
    assert_eq!((a.line, a.ch), (b.line, b.ch));
}

/// `testBasic(a, b)`.
fn test_basic(a: &Doc, b: &Doc) {
    eq_all("x", &[a, b]);
    a.set_value("hey");
    eq_all("hey", &[a, b]);
    b.set_value("wow");
    eq_all("wow", &[a, b]);
    a.replace_range("u\nv\nw", p(0, 3), None, None);
    b.replace_range("i", p(0, 4), None, None);
    b.replace_range("j", p(2, 1), None, None);
    eq_all("wowui\nv\nwj", &[a, b]);
}

#[test]
fn doc_basic() {
    let e = spec("A='x' B<A");
    test_basic(&e[0], &e[1]);
}

#[test]
fn doc_basic_separate() {
    // testDoc("basicSeparate", ...)
    let e = spec("A='x' B<~A");
    test_basic(&e[0], &e[1]);
}

#[test]
fn doc_shared_hist() {
    // testDoc("sharedHist", ...)
    let e = spec("A='ab\ncd\nef' B<A");
    let (a, b) = (&e[0], &e[1]);
    a.replace_range("x", pl(0), None, None);
    b.replace_range("y", pl(1), None, None);
    a.replace_range("z", pl(2), None, None);
    eq_all("abx\ncdy\nefz", &[a, b]);
    a.undo();
    a.undo();
    eq_all("abx\ncd\nef", &[a, b]);
    a.redo();
    eq_all("abx\ncdy\nef", &[a, b]);
    b.redo();
    eq_all("abx\ncdy\nefz", &[a, b]);
    a.undo();
    b.undo();
    a.undo();
    a.undo();
    eq_all("ab\ncd\nef", &[a, b]);
}

#[test]
fn doc_undo_intact() {
    // testDoc("undoIntact", ...)
    let e = spec("A='ab\ncd\nef' B<~A");
    let (a, b) = (&e[0], &e[1]);
    a.replace_range("x", pl(0), None, None);
    b.replace_range("y", pl(1), None, None);
    a.replace_range("z", pl(2), None, None);
    a.replace_range("q", pl(0), None, None);
    eq_all("abxq\ncdy\nefz", &[a, b]);
    a.undo();
    a.undo();
    eq_all("abx\ncdy\nef", &[a, b]);
    b.undo();
    eq_all("abx\ncd\nef", &[a, b]);
    a.redo();
    eq_all("abx\ncd\nefz", &[a, b]);
    a.redo();
    eq_all("abxq\ncd\nefz", &[a, b]);
    a.undo();
    a.undo();
    a.undo();
    a.undo();
    eq_all("ab\ncd\nef", &[a, b]);
    b.redo();
    eq_all("ab\ncdy\nef", &[a, b]);
}

#[test]
fn doc_undo_conflict() {
    // testDoc("undoConflict", ...)
    let e = spec("A='ab\ncd\nef' B<~A");
    let (a, b) = (&e[0], &e[1]);
    a.replace_range("x", pl(0), None, None);
    a.replace_range("z", pl(2), None, None);
    // This should clear the first undo event in a, but not the second
    b.replace_range("y", pl(0), None, None);
    a.undo();
    a.undo();
    eq_all("abxy\ncd\nef", &[a, b]);
    a.replace_range("u", pl(2), None, None);
    a.replace_range("v", pl(0), None, None);
    // This should clear both events in a
    b.replace_range("w", pl(0), None, None);
    a.undo();
    a.undo();
    eq_all("abxyvw\ncd\nefu", &[a, b]);
}

#[test]
fn doc_double_rebase() {
    // testDoc("doubleRebase", ...)
    let e = spec("A='ab\ncd\nef\ng' B<~A C<B");
    let (a, b, c) = (&e[0], &e[1], &e[2]);
    c.replace_range("u", pl(3), None, None);
    a.replace_range("", p(0, 0), Some(p(1, 0)), None);
    c.undo();
    eq_all("cd\nef\ng", &[a, b, c]);
}

#[test]
fn doc_undo_update() {
    // testDoc("undoUpdate", ...)
    let e = spec("A='ab\ncd\nef' B<~A");
    let (a, b) = (&e[0], &e[1]);
    a.replace_range("x", pl(2), None, None);
    b.replace_range("u\nv\nw\n", p(0, 0), None, None);
    a.undo();
    eq_all("u\nv\nw\nab\ncd\nef", &[a, b]);
    a.redo();
    eq_all("u\nv\nw\nab\ncd\nefx", &[a, b]);
    a.undo();
    eq_all("u\nv\nw\nab\ncd\nef", &[a, b]);
    b.undo();
    a.redo();
    eq_all("ab\ncd\nefx", &[a, b]);
    a.undo();
    eq_all("ab\ncd\nef", &[a, b]);
}

#[test]
fn doc_undo_keep_ranges() {
    // testDoc("undoKeepRanges", ...)
    let e = spec("A='abcdefg' B<A");
    let (a, b) = (&e[0], &e[1]);
    let m = a
        .mark_text(
            p(0, 1),
            p(0, 3),
            MarkOptions {
                class_name: Some("foo".into()),
                ..Default::default()
            },
        )
        .unwrap();
    b.replace_range("x", p(0, 0), None, None);
    eq_char_pos(from_of(&m), p(0, 2));
    b.replace_range("yzzy", p(0, 1), Some(pl(0)), None);
    assert!(m.find().is_none());
    b.undo();
    eq_char_pos(from_of(&m), p(0, 2));
    b.undo();
    eq_char_pos(from_of(&m), p(0, 1));
}

#[test]
fn doc_long_chain() {
    // testDoc("longChain", ...)
    let e = spec("A='uv' B<A C<B D<C");
    let (a, b, c, d) = (&e[0], &e[1], &e[2], &e[3]);
    a.replace_selection("X", None, None);
    eq_all("Xuv", &[a, b, c, d]);
    d.replace_range("Y", pl(0), None, None);
    eq_all("XuvY", &[a, b, c, d]);
}

#[test]
fn doc_broad_cast() {
    // testDoc("broadCast", ...)
    let e = spec("B<A C<A D<A E<A");
    let (a, b, c, d, ee) = (&e[0], &e[1], &e[2], &e[3], &e[4]);
    b.set_value("uu");
    eq_all("uu", &[a, b, c, d, ee]);
    a.replace_range("v", p(0, 1), None, None);
    eq_all("uvu", &[a, b, c, d, ee]);
}

#[test]
fn doc_islands() {
    // testDoc("islands", ...): A and B share a history, C and D share a separate one
    let e = spec("A='x\ny\nz' B<A C<~A D<C");
    let (a, b, c, d) = (&e[0], &e[1], &e[2], &e[3]);
    a.replace_range("u", pl(0), None, None);
    d.replace_range("v", pl(2), None, None);
    b.undo();
    eq_all("x\ny\nzv", &[a, b, c, d]);
    c.undo();
    eq_all("x\ny\nz", &[a, b, c, d]);
    a.redo();
    eq_all("xu\ny\nz", &[a, b, c, d]);
    d.redo();
    eq_all("xu\ny\nzv", &[a, b, c, d]);
}

#[test]
fn doc_unlink() {
    // testDoc("unlink", ...)
    let e = spec("B<A C<A D<B");
    let (a, b, c, d) = (&e[0], &e[1], &e[2], &e[3]);
    a.set_value("hi");
    b.unlink_doc(a);
    d.set_value("aye");
    eq_all("hi", &[a, c]);
    eq_all("aye", &[b, d]);
    a.set_value("oo");
    eq_all("oo", &[a, c]);
    eq_all("aye", &[b, d]);
}

#[test]
fn doc_bare_doc() {
    // testDoc("bareDoc", ...). The `instanceof` checks become editor/no-editor.
    let e = spec("A*='foo' B*<A C<B");
    let (a, b, c) = (&e[0], &e[1], &e[2]);
    eq_all("foo", &[a, b, c]);
    a.replace_range("hey", p(0, 0), Some(pl(0)), None);
    c.replace_range("!", pl(0), None, None);
    eq_all("hey!", &[a, b, c]);
    b.unlink_doc(a);
    b.set_value("x");
    eq_all("x", &[b, c]);
    eq_all("hey!", &[a]);
}

#[test]
fn doc_copy_doc() {
    // testDoc("copyDoc", ...): `a.swapDoc(copy)` only routes the calls to `copy`.
    let e = spec("A='u'");
    let a = &e[0];
    let copy = editor(a.copy(true));
    a.set_value("foo");
    copy.set_value("bar");
    assert_eq!(copy.get_value(None), "bar");
    copy.undo();
    assert_eq!(copy.get_value(None), "u");
    assert_eq!(a.get_value(None), "foo");
    assert_eq!(a.history_size().undo, 1);
    assert_eq!(a.copy(false).history_size().undo, 0);
}

#[test]
fn doc_subview() {
    // testDoc("subview", ...)
    let e = spec("A='1\n2\n3\n4\n5' B<~A/1-3");
    let (a, b) = (&e[0], &e[1]);
    assert_eq!(b.get_value(None), "2\n3");
    assert_eq!(b.first_line(), 1);
    b.set_cursor(pl(4), None);
    eq_char_pos(b.get_cursor(None), p(2, 1));
    a.replace_range("-1\n0\n", p(0, 0), None, None);
    assert_eq!(b.first_line(), 3);
    eq_char_pos(b.get_cursor(None), p(4, 1));
    a.undo();
    eq_char_pos(b.get_cursor(None), p(2, 1));
    b.replace_range("oyoy\n", p(2, 0), None, None);
    assert_eq!(a.get_value(None), "1\n2\noyoy\n3\n4\n5");
    b.undo();
    assert_eq!(a.get_value(None), "1\n2\n3\n4\n5");
}

#[test]
fn doc_subview_edit_on_boundary() {
    // testDoc("subviewEditOnBoundary", ...)
    let e = spec("A='11\n22\n33\n44\n55' B<~A/1-4");
    let (a, b) = (&e[0], &e[1]);
    a.replace_range("x\nyy\nz", p(0, 1), Some(p(2, 1)), None);
    assert_eq!(b.first_line(), 2);
    assert_eq!(b.line_count(), 2);
    assert_eq!(b.get_value(None), "z3\n44");
    a.replace_range("q\nrr\ns", p(3, 1), Some(p(4, 1)), None);
    assert_eq!(b.first_line(), 2);
    assert_eq!(b.get_value(None), "z3\n4q");
    assert_eq!(a.get_value(None), "1x\nyy\nz3\n4q\nrr\ns5");
    a.select_all(); // a.execCommand("selectAll")
    a.replace_selection("!", None, None);
    eq_all("!", &[a, b]);
}

#[test]
fn doc_shared_marker() {
    // testDoc("sharedMarker", ...)
    let e = spec("A='ab\ncd\nef\ngh' B<A C<~A/1-2");
    let (a, b, c) = (&e[0], &e[1], &e[2]);
    let mark = b
        .mark_text(
            p(0, 1),
            p(3, 1),
            MarkOptions {
                class_name: Some("cm-searching".into()),
                shared: true,
                ..Default::default()
            },
        )
        .unwrap();
    let found = a.find_marks_at(p(0, 2));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0], mark);
    assert_eq!(c.find_marks_at(p(1, 1)).len(), 1);
    eq_char_pos(from_of(&mark), p(0, 1));
    eq_char_pos(to_of(&mark), p(3, 1));
    b.replace_range("x\ny\n", p(0, 0), None, None);
    eq_char_pos(from_of(&mark), p(2, 1));
    eq_char_pos(to_of(&mark), p(5, 1));
    let cleared = Rc::new(Cell::new(0));
    let Marker::Shared(sm) = &mark else {
        panic!("shared")
    };
    let c2 = cleared.clone();
    sm.on_clear(move || c2.set(c2.get() + 1));
    b.operation(|| mark.clear());
    assert_eq!(a.find_marks_at(p(3, 1)).len(), 0);
    assert_eq!(b.find_marks_at(p(3, 1)).len(), 0);
    assert_eq!(c.find_marks_at(p(3, 1)).len(), 0);
    assert!(mark.find().is_none());
    assert_eq!(cleared.get(), 1);
}

#[test]
fn doc_shared_marker_copy() {
    // testDoc("sharedMarkerCopy", ...)
    let e = spec("A='abcde'");
    let a = &e[0];
    let shared = a
        .mark_text(
            p(0, 1),
            p(0, 3),
            MarkOptions {
                shared: true,
                ..Default::default()
            },
        )
        .unwrap();
    let b = a.linked_doc(LinkedDocOptions::default()).unwrap();
    let found = b.find_marks_at(p(0, 2));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0], shared);
    shared.clear();
    // eq(b.findMarksAt(Pos(0, 2)), 0) compares [] == 0 loosely: empty.
    assert!(b.find_marks_at(p(0, 2)).is_empty());
}

#[test]
fn doc_shared_marker_detach() {
    // testDoc("sharedMarkerDetach", ...)
    let e = spec("A='abcde' B<A C<B");
    let (a, b, c) = (&e[0], &e[1], &e[2]);
    let shared = a
        .mark_text(
            p(0, 1),
            p(0, 3),
            MarkOptions {
                shared: true,
                ..Default::default()
            },
        )
        .unwrap();
    a.unlink_doc(b);
    let in_b = b.find_marks_at(p(0, 2));
    assert_eq!(in_b.len(), 1);
    assert!(in_b[0] != shared);
    let in_c = c.find_marks_at(p(0, 2));
    assert_eq!(in_c.len(), 1);
    assert!(in_c[0] != shared);
    in_c[0].clear();
    assert!(shared.find().is_some());
}

#[test]
fn doc_shared_bookmark() {
    // testDoc("sharedBookmark", ...)
    let e = spec("A='ab\ncd\nef\ngh' B<A C<~A/1-2");
    let (a, b, c) = (&e[0], &e[1], &e[2]);
    let mark = b.set_bookmark(p(1, 1), false, true).unwrap();
    let found = a.find_marks_at(p(1, 1));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0], mark);
    assert_eq!(c.find_marks_at(p(1, 1)).len(), 1);
    assert!(matches!(mark.find(), Some(MarkerFind::Point(q)) if (q.line, q.ch) == (1, 1)));
    b.replace_range("x\ny\n", p(0, 0), None, None);
    assert!(matches!(mark.find(), Some(MarkerFind::Point(q)) if (q.line, q.ch) == (3, 1)));
    let cleared = Rc::new(Cell::new(0));
    let Marker::Shared(sm) = &mark else {
        panic!("shared")
    };
    let c2 = cleared.clone();
    sm.on_clear(move || c2.set(c2.get() + 1));
    b.operation(|| mark.clear());
    assert_eq!(a.find_marks(p(0, 0), pl(5), None).len(), 0);
    assert_eq!(b.find_marks(p(0, 0), pl(5), None).len(), 0);
    assert_eq!(c.find_marks(p(0, 0), pl(5), None).len(), 0);
    assert!(mark.find().is_none());
    assert_eq!(cleared.get(), 1);
}

#[test]
fn doc_undo_in_subview() {
    // testDoc("undoInSubview", ...)
    let e = spec("A='line 0\nline 1\nline 2\nline 3\nline 4' B<A/1-4");
    let (a, b) = (&e[0], &e[1]);
    b.replace_range("x", p(2, 0), None, None);
    a.undo();
    assert_eq!(a.get_value(None), "line 0\nline 1\nline 2\nline 3\nline 4");
    assert_eq!(b.get_value(None), "line 1\nline 2\nline 3");
}
