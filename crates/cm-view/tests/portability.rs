//! L-B portability boundaries on the editor: the clock contract through `Editor`
//! (construction, typing, undo/redo, indentation, cut/paste origins), the file
//! host behind `App`, and the clipboard feature split.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use cm_core::clock::{self, ClockStatus};
use cm_modes::Spec;
use cm_view::app::{App, AppOptions, MemClipboard};
use cm_view::editor::Indent;
use cm_view::fileio::{self, FileHost, MemFiles, StdFs};
use cm_view::native_clipboard::SystemClipboard;
use cm_view::{Editor, EditorOptions, Metrics};

fn editor(text: &str, mode: &str) -> Editor {
    Editor::new(
        text,
        Spec::from(mode),
        EditorOptions::default(),
        Metrics::test_grid(),
    )
}

fn origins(ed: &Editor) -> Rc<RefCell<Vec<String>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s = seen.clone();
    ed.on_change(move |_cx, ch| {
        s.borrow_mut()
            .push(ch.origin.clone().unwrap_or_else(|| "(none)".into()))
    });
    seen
}

/// Type, smart-indent, undo once and redo. Returns the undo depth after typing and
/// the text after the one undo. `indentLine` uses origin `+input`, so inside the
/// merge window it joins the typing event, as in CM5.
fn edit_cycle(ed: &mut Editor) -> (usize, String) {
    ed.doc.set_cursor(cm_core::Pos::new(1, 0), None);
    for c in "x = 1;".chars() {
        ed.type_text(&c.to_string());
    }
    let typed = ed.doc.history_size().undo;
    ed.indent_line(1, Some(Indent::Smart), true);
    assert_eq!(ed.get_value(), "int f() {\n  x = 1;\n}");
    assert!(ed.exec_command("undo"));
    let undone = ed.get_value();
    assert!(ed.exec_command("redo"));
    assert_eq!(ed.get_value(), "int f() {\n  x = 1;\n}");
    (typed, undone)
}

const EMPTY: &str = "int f() {\n\n}";
const UNINDENTED: &str = "int f() {\nx = 1;\n}";

#[test]
fn native_editor_uses_the_system_clock() {
    assert_eq!(clock::status(), ClockStatus::System);
    let mut ed = editor("int f() {\n\n}", "text/x-csrc");
    let (typed, undone) = edit_cycle(&mut ed);
    assert!((1..=6).contains(&typed));
    assert!(undone == EMPTY || undone == UNINDENTED);
    assert_eq!(clock::missing_reads(), 0);
}

#[test]
fn host_clock_editor_merges_typing() {
    let t = clock::manual(1e12);
    let mut ed = editor("int f() {\n\n}", "text/x-csrc");
    assert_eq!(
        edit_cycle(&mut ed),
        (1, EMPTY.to_string()),
        "frozen clock: typing and indentation are one event"
    );
    // The editor's historyEventDelay is 1250 ms (CM5 default); the test is strict.
    let mut ed = editor("", "text/plain");
    ed.type_text("a");
    t.set(t.get() + 1249.0);
    ed.type_text("b");
    assert_eq!(ed.doc.history_size().undo, 1);
    t.set(t.get() + 1250.0);
    ed.type_text("c");
    assert_eq!(ed.doc.history_size().undo, 2);
}

#[test]
fn unknown_time_editor_works_and_splits_typing() {
    clock::set_host_clock(Some(Box::new(|| None)));
    let mut ed = editor("int f() {\n\n}", "text/x-csrc");
    assert_eq!(
        edit_cycle(&mut ed),
        (6, UNINDENTED.to_string()),
        "one event per keystroke, indentation separate"
    );
    for _ in 0..7 {
        assert!(ed.exec_command("undo"));
    }
    assert_eq!(ed.get_value(), "int f() {\n\n}");
    assert!(clock::missing_reads() > 0);
}

#[test]
fn unknown_time_keeps_cut_and_paste_origins() {
    clock::set_host_clock(Some(Box::new(|| None)));
    let mut ed = editor("hello world", "text/plain");
    let seen = origins(&ed);
    ed.doc
        .set_selection(cm_core::Pos::new(0, 0), Some(cm_core::Pos::new(0, 6)), None);
    assert_eq!(ed.cut().as_deref(), Some("hello "));
    ed.paste("hi ");
    ed.type_text("!");
    assert_eq!(ed.get_value(), "hi !world");
    assert_eq!(*seen.borrow(), ["cut", "paste", "+input"]);
}

#[test]
fn unknown_time_still_merges_one_operation() {
    clock::set_host_clock(Some(Box::new(|| None)));
    let mut ed = editor("", "text/plain");
    ed.run_op(|cm| {
        cm.type_text("a");
        cm.type_text("b");
    });
    assert_eq!(ed.doc.history_size().undo, 1, "same operation id merges");
}

#[test]
fn repeated_construction_and_unicode() {
    clock::manual(1e12);
    for i in 0..100 {
        let mut ed = editor("ü😀\n日本", "text/plain");
        ed.doc.set_cursor(cm_core::Pos::new(0, 3), None);
        ed.type_text("é");
        assert_eq!(ed.get_value(), "ü😀é\n日本", "iteration {i}");
        assert!(ed.exec_command("undo"));
        assert_eq!(ed.get_value(), "ü😀\n日本");
    }
}

// ---- file host ----

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cm-view-lb-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn std_fs_matches_load_and_save() {
    let dir = scratch("stdfs");
    let path = dir.join("a.wrl");
    let bytes = b"#VRML V2.0 utf8\r\nShape {}\r\n";
    std::fs::write(&path, bytes).unwrap();
    let mut fs = StdFs;
    let got = fs.read(&path).unwrap().unwrap();
    assert_eq!(got, bytes);
    let (t1, i1) = fileio::load(&path).unwrap();
    let (t2, i2) = fileio::decode(&path, &got).unwrap();
    assert_eq!((t1, i1), (t2.clone(), i2.clone()));
    fs.write(&path, &fileio::encode(&t2, &i2).unwrap()).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(fs.read(&dir.join("missing.wrl")).unwrap(), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn app_opens_and_saves_through_a_memory_host() {
    clock::manual(1e12);
    let path = Path::new("/virtual/doc.wrl");
    let mut files = MemFiles::default();
    files.0.insert(path.to_path_buf(), b"caf\xe9\n".to_vec());
    let mut app = App::open_with(
        Box::new(files),
        path,
        Metrics::test_grid(),
        &AppOptions::default(),
        Box::new(MemClipboard::default()),
    )
    .unwrap();
    assert_eq!(app.editor.get_value(), "caf\u{e9}\n");
    app.editor.doc.set_cursor(cm_core::Pos::new(1, 0), None);
    app.editor.type_text("x");
    app.save().unwrap();
    assert!(app.editor.doc.is_clean(None));
    assert_eq!(
        app.files.read(path).unwrap().as_deref(),
        Some(&b"caf\xe9\nx"[..]),
        "Latin-1 written back as Latin-1"
    );
    assert!(!Path::new("/virtual").exists(), "no filesystem access");
}

#[test]
fn app_open_missing_file_is_empty_and_created_on_save() {
    clock::manual(1e12);
    let path = Path::new("/virtual/new.txt");
    let mut app = App::open_with(
        Box::new(MemFiles::default()),
        path,
        Metrics::test_grid(),
        &AppOptions::default(),
        Box::new(MemClipboard::default()),
    )
    .unwrap();
    assert_eq!(app.editor.get_value(), "");
    app.editor.type_text("hi");
    app.save().unwrap();
    assert_eq!(app.files.read(path).unwrap().as_deref(), Some(&b"hi"[..]));
}

#[test]
fn app_open_keeps_native_filesystem_behavior() {
    clock::manual(1e12);
    let dir = scratch("app-open");
    let path = dir.join("n.txt");
    let mut app = App::open(
        &path,
        Metrics::test_grid(),
        &AppOptions::default(),
        Box::new(MemClipboard::default()),
    )
    .unwrap();
    app.editor.type_text("a\r\nb");
    app.save().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"a\nb");
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---- clipboard ----

#[test]
fn in_memory_clipboard_round_trips() {
    use cm_view::app::Clipboard;
    let mut c = SystemClipboard::in_memory();
    assert!(!c.is_system());
    assert_eq!(c.get(), None);
    c.set("x");
    assert_eq!(c.get().as_deref(), Some("x"));
}

// ---- modes (the wasm probe checks the same values on wasm32) ----

#[test]
fn registered_mode_tokens_match_the_wasm_probe() {
    clock::manual(1e12);
    let mut reg = cm_modes::Registry::new();
    assert!(!reg.has_mode("toml"));
    cm_modes::toml::register(&mut reg);
    let toks: Vec<String> =
        cm_modes::run_mode_collect(&reg, "[a]\nk = \"ü😀\" # c", &Spec::from("text/x-toml"))
            .into_iter()
            .map(|(t, s)| format!("{t}|{}", s.unwrap_or_default()))
            .collect();
    assert_eq!(
        toks,
        [
            "[a]|atom",
            "\n|",
            "k|property",
            " |",
            "=|",
            " |",
            "\"ü😀\"|string",
            " |",
            "# c|comment"
        ]
    );
    let mut ed = Editor::with_registry(
        "x = 1",
        Spec::from("text/plain"),
        EditorOptions::default(),
        Metrics::test_grid(),
        Rc::new(reg),
    );
    assert_eq!(ed.mode_name(), "null");
    ed.set_mode(Spec::from("text/x-toml"));
    assert_eq!(ed.mode_name(), "toml");
    let first = ed.get_line_tokens(0, true).into_iter().next().unwrap();
    assert_eq!(
        (first.string.as_str(), first.ty.as_deref()),
        ("x", Some("property"))
    );
}
