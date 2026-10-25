//! Scripted input through the application's input layer (`cm_view::app::App`):
//! typing, Enter with smart indentation, an electric `}`, smart Home, shift-motion
//! selection, undo, copy/paste, and Ctrl-S saving to a /tmp file whose bytes are
//! checked. With the `gpu` feature, the same script also drives the real winit
//! event loop of `cm-edit` under `xvfb-run -a` (no window on the desktop).

use std::path::{Path, PathBuf};

use cm_core::Pos;
use cm_view::app::{App, AppOptions, Input, MemClipboard, Step, parse_script};
use cm_view::{KeyEvent, Metrics};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn tmp_copy(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cm-view-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("work.wrl");
    std::fs::copy(data("edit-input.wrl"), &p).unwrap();
    p
}

#[test]
fn scripted_edit_and_save() {
    let clock = cm_core::clock::manual(1e12);
    let path = tmp_copy("app");
    let mut app = App::open(
        &path,
        Metrics::test_grid(),
        &AppOptions::default(),
        Box::new(MemClipboard::default()),
    )
    .unwrap();
    assert_eq!(app.editor.mode_name(), "vrml");
    let steps = parse_script(&std::fs::read_to_string(data("edit-script.txt")).unwrap()).unwrap();
    for s in steps {
        match s {
            Step::Input(evs) => {
                for e in evs {
                    app.handle(e);
                    clock.set(clock.get() + 30.0);
                }
            }
            Step::Pause(ms) => clock.set(clock.get() + ms as f64),
            Step::Screenshot(_) | Step::Quit => {}
        }
    }
    assert_eq!(app.status, format!("saved {}", path.display()));
    let saved = std::fs::read(&path).unwrap();
    let expected = std::fs::read(data("edit-expected.wrl")).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&saved),
        String::from_utf8_lossy(&expected)
    );
    assert!(app.editor.doc.is_clean(None));
    assert!(!app.title().contains('\u{2022}'));
}

#[test]
fn read_only_blocks_edits_but_not_motion() {
    let _clock = cm_core::clock::manual(1e12);
    let path = tmp_copy("ro");
    let mut app = App::open(
        &path,
        Metrics::test_grid(),
        &AppOptions {
            read_only: true,
            ..Default::default()
        },
        Box::new(MemClipboard::default()),
    )
    .unwrap();
    let before = app.editor.get_value();
    app.handle(Input::Key {
        key: KeyEvent::parse("Down"),
        text: None,
    });
    app.handle(Input::Key {
        key: KeyEvent::default(),
        text: Some("x".into()),
    });
    app.handle(Input::Key {
        key: KeyEvent::parse("Enter"),
        text: Some("\r".into()),
    });
    app.handle(Input::Key {
        key: KeyEvent::parse("Backspace"),
        text: None,
    });
    app.handle(Input::Key {
        key: KeyEvent::parse("Ctrl-D"),
        text: None,
    });
    app.handle(Input::Key {
        key: KeyEvent::parse("Ctrl-V"),
        text: None,
    });
    assert_eq!(app.editor.get_value(), before);
    assert_eq!(
        app.editor.doc.get_cursor(None),
        Pos::with_sticky(1, 0, Some(cm_core::Sticky::After))
    );
    app.handle(Input::Key {
        key: KeyEvent::parse("Ctrl-S"),
        text: None,
    });
    assert_eq!(app.status, "save failed: read-only");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        std::fs::read(data("edit-input.wrl")).unwrap()
    );
    assert!(app.title().contains("[read-only]"));
}

#[test]
fn mouse_click_drag_double_click_shift_click() {
    let clock = cm_core::clock::manual(1e12);
    let path = tmp_copy("mouse");
    let mut app = App::open(
        &path,
        Metrics::test_grid(),
        &AppOptions::default(),
        Box::new(MemClipboard::default()),
    )
    .unwrap();
    app.handle(Input::Resize {
        width: 600.0,
        height: 400.0,
    });
    let g = app.editor.gutter_width;
    // Window coordinates of a character: gutter + padding + col*10, line*20 + padding.
    let at = |line: f64, col: f64| (g + 4.0 + col * 10.0 + 1.0, 4.0 + line * 20.0 + 10.0);
    let none = KeyEvent::default();
    // Click on line 2 ("  geometry Box ..."), column 4.
    let (x, y) = at(2.0, 4.0);
    app.handle(Input::MouseDown {
        x,
        y,
        button: 1,
        mods: none.clone(),
        time: 1000.0,
    });
    app.handle(Input::MouseUp { x, y });
    assert_eq!(
        app.editor.doc.list_selections()[0].head,
        Pos::with_sticky(2, 4, Some(cm_core::Sticky::After))
    );
    // Drag to column 10 on the same line.
    let (x2, _) = at(2.0, 10.0);
    app.handle(Input::MouseDown {
        x,
        y,
        button: 1,
        mods: none.clone(),
        time: 3000.0,
    });
    app.handle(Input::MouseMove {
        x: x2,
        y,
        mods: none.clone(),
    });
    app.handle(Input::MouseUp { x: x2, y });
    let r = app.editor.doc.list_selections()[0];
    assert_eq!((r.anchor.ch, r.head.ch), (4, 10));
    // Double click selects the word "geometry" (columns 2..10).
    let (xw, _) = at(2.0, 5.0);
    app.handle(Input::MouseDown {
        x: xw,
        y,
        button: 1,
        mods: none.clone(),
        time: 6000.0,
    });
    app.handle(Input::MouseUp { x: xw, y });
    app.handle(Input::MouseDown {
        x: xw,
        y,
        button: 1,
        mods: none.clone(),
        time: 6100.0,
    });
    app.handle(Input::MouseUp { x: xw, y });
    assert_eq!(app.editor.doc.get_selection(None), "geometry");
    // Shift-click on line 3 extends from the anchor.
    let (x3, y3) = at(3.0, 1.0);
    let shift = KeyEvent {
        shift: true,
        ..Default::default()
    };
    app.handle(Input::MouseDown {
        x: x3,
        y: y3,
        button: 1,
        mods: shift,
        time: 9000.0,
    });
    app.handle(Input::MouseUp { x: x3, y: y3 });
    assert_eq!(
        app.editor.doc.get_selection(None),
        "geometry Box { size 1 1 1 }\n}"
    );
    // Clicks in the gutter do nothing.
    let before = app.editor.doc.list_selections();
    app.handle(Input::MouseDown {
        x: 2.0,
        y,
        button: 1,
        mods: KeyEvent::default(),
        time: 12000.0,
    });
    assert_eq!(app.editor.doc.list_selections(), before);
    clock.set(clock.get() + 1.0);
}

#[test]
fn wheel_scroll_is_clamped_and_virtualized() {
    let _clock = cm_core::clock::manual(1e12);
    let text: String = (0..5000).map(|i| format!("line {i}\n")).collect();
    let mut app = App::new(
        &text,
        None,
        Metrics::test_grid(),
        &AppOptions::default(),
        Box::new(MemClipboard::default()),
    );
    app.handle(Input::Resize {
        width: 600.0,
        height: 400.0,
    });
    app.handle(Input::Wheel {
        dx: 0.0,
        dy: 20.0 * 1000.0,
    });
    let (from, to) = app.editor.visible_lines();
    assert_eq!(from, 999);
    assert!(to - from <= 22);
    assert!(app.editor.display.view_to - app.editor.display.view_from <= 42);
    app.handle(Input::Wheel { dx: 0.0, dy: 1e9 });
    let (mh, _) = app.editor.max_scroll();
    assert_eq!(app.editor.display.scroll_top, mh);
    // PageDown from the top moves the cursor by a page and scrolls.
    app.handle(Input::Wheel { dx: 0.0, dy: -1e9 });
    app.handle(Input::Key {
        key: KeyEvent::parse("PageDown"),
        text: None,
    });
    // findPosV: y = bottom of line 0 (20) + (pageSize 400 - textHeight/2) = 410 -> line 20.
    assert_eq!(app.editor.doc.get_cursor(None).line, 20);
    assert!(app.editor.display.scroll_top > 0.0);
}

/// The same script through the winit event loop of the real binary, on a private
/// X display. Skipped when `xvfb-run` is missing.
#[cfg(feature = "gpu")]
#[test]
fn winit_event_loop_under_xvfb() {
    if std::process::Command::new("which")
        .arg("xvfb-run")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        return x3d_evidence::tool_absent("xvfb-run not found");
    }
    let path = tmp_copy("xvfb");
    let shot = path.with_file_name("after.png");
    let script = std::fs::read_to_string(data("edit-script.txt"))
        .unwrap()
        .replace("quit", &format!("screenshot {}\nquit", shot.display()));
    let script_path = path.with_file_name("steps.txt");
    std::fs::write(&script_path, script).unwrap();
    let out = std::process::Command::new("timeout")
        .args(["60", "xvfb-run", "-a", env!("CARGO_BIN_EXE_cm-edit")])
        .arg(&path)
        .arg("--script")
        .arg(&script_path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "cm-edit failed: {}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let saved = std::fs::read(&path).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&saved),
        String::from_utf8_lossy(&std::fs::read(data("edit-expected.wrl")).unwrap())
    );
    assert!(shot.exists());
    println!("xvfb run ok; screenshot {}", shot.display());
}
