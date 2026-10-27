//! The file-host read contract (C-HOST-FILE, R6). A file is missing only when the
//! operating system reports `NotFound`; every other read error reaches the caller
//! unchanged, and `App::open_with` then builds no editor, so a failed read never
//! becomes an empty document. The encoding round trips through `StdFs` and `App`
//! are kept here too.
//!
//! Each test works in its own directory under the system temp directory and
//! removes it (restoring permissions first) when it ends.

use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use cm_core::clock;
use cm_view::Metrics;
use cm_view::app::{App, AppOptions, MemClipboard};
use cm_view::fileio::{self, Encoding, FileHost, MemFiles, StdFs};

/// A directory owned by one test, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("cm-view-host-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        #[cfg(unix)]
        restore_permissions(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn restore_permissions(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755));
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir()) {
                restore_permissions(&p);
            } else if !std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
                let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644));
            }
        }
    }
}

fn open(files: Box<dyn FileHost>, path: &Path) -> std::io::Result<App> {
    clock::manual(1e12);
    App::open_with(
        files,
        path,
        Metrics::test_grid(),
        &AppOptions::default(),
        Box::new(MemClipboard::default()),
    )
}

fn open_std(path: &Path) -> std::io::Result<App> {
    clock::manual(1e12);
    App::open(
        path,
        Metrics::test_grid(),
        &AppOptions::default(),
        Box::new(MemClipboard::default()),
    )
}

/// `StdFs.read` fails exactly as `std::fs::read` does: same kind, same OS error.
fn assert_same_error(path: &Path) -> std::io::Error {
    let want = std::fs::read(path).expect_err("std::fs::read should fail");
    assert_ne!(want.kind(), ErrorKind::NotFound, "{want}");
    let got = StdFs
        .read(path)
        .expect_err("StdFs.read must report the error, not a missing file");
    assert_eq!(got.kind(), want.kind(), "{got} vs {want}");
    assert_eq!(got.raw_os_error(), want.raw_os_error(), "{got} vs {want}");
    let app = open_std(path).err().expect("App::open must fail");
    assert_eq!(app.kind(), want.kind(), "{app}");
    assert_eq!(app.raw_os_error(), want.raw_os_error(), "{app}");
    got
}

// ---- StdFs: reads ----

#[test]
fn readable_file_is_read() {
    let s = Scratch::new("readable");
    let p = s.file("a.txt", b"one\ntwo\n");
    assert_eq!(StdFs.read(&p).unwrap().as_deref(), Some(&b"one\ntwo\n"[..]));
}

#[test]
fn missing_file_is_none_and_opens_empty() {
    let s = Scratch::new("missing");
    for p in [s.0.join("none.wrl"), s.0.join("no-dir").join("none.wrl")] {
        assert_eq!(std::fs::read(&p).unwrap_err().kind(), ErrorKind::NotFound);
        assert_eq!(StdFs.read(&p).unwrap(), None, "{}", p.display());
        let app = open_std(&p).unwrap();
        assert_eq!(app.editor.get_value(), "");
        assert!(app.editor.doc.is_clean(None));
        let f = app.file.as_ref().unwrap();
        assert_eq!(
            (&f.path, f.gzip, f.encoding, &f.line_separator),
            (&p, false, Encoding::Utf8, &None)
        );
    }
}

#[test]
fn a_path_with_a_nul_byte_is_an_error() {
    let p = Path::new("bad\0name.txt");
    let e = assert_same_error(p);
    assert_eq!(e.kind(), ErrorKind::InvalidInput);
}

#[test]
fn a_directory_is_an_error() {
    let s = Scratch::new("is-dir");
    std::fs::create_dir(s.0.join("d")).unwrap();
    assert_same_error(&s.0.join("d"));
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn chmod(p: &Path, mode: u32) {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// A process that reads through mode 000 (root, CAP_DAC_OVERRIDE) cannot test
    /// permission errors.
    fn privileged(p: &Path) -> bool {
        std::fs::read(p).is_ok()
    }

    #[test]
    fn unreadable_file_is_permission_denied() {
        let s = Scratch::new("perm-file");
        let p = s.file("secret.wrl", b"#VRML V2.0 utf8\n");
        chmod(&p, 0o000);
        if privileged(&p) {
            return cm_evidence::tool_absent("an unprivileged process (mode 000 was readable)");
        }
        let e = assert_same_error(&p);
        assert_eq!(e.kind(), ErrorKind::PermissionDenied);
        chmod(&p, 0o644);
        assert_eq!(
            std::fs::read(&p).unwrap(),
            b"#VRML V2.0 utf8\n",
            "file intact"
        );
    }

    #[test]
    fn unsearchable_directory_is_permission_denied() {
        let s = Scratch::new("perm-dir");
        let dir = s.0.join("locked");
        std::fs::create_dir(&dir).unwrap();
        let p = dir.join("a.wrl");
        std::fs::write(&p, b"keep").unwrap();
        chmod(&dir, 0o000);
        if privileged(&p) {
            return cm_evidence::tool_absent("an unprivileged process (mode 000 was searchable)");
        }
        // `Path::exists` is false here, which the old `StdFs.read` took for "missing".
        assert!(!p.exists());
        let e = assert_same_error(&p);
        assert_eq!(e.kind(), ErrorKind::PermissionDenied);
        // A file that really is missing behind the same directory is also an error:
        // the OS reports permission, not absence.
        let e = assert_same_error(&dir.join("never-created.wrl"));
        assert_eq!(e.kind(), ErrorKind::PermissionDenied);
        chmod(&dir, 0o755);
        assert_eq!(std::fs::read(&p).unwrap(), b"keep", "file intact");
    }

    #[test]
    fn symlink_loop_is_not_missing() {
        let s = Scratch::new("loop");
        let a = s.0.join("a.wrl");
        let b = s.0.join("b.wrl");
        symlink(&b, &a).unwrap();
        symlink(&a, &b).unwrap();
        let own = s.0.join("self.wrl");
        symlink(&own, &own).unwrap();
        for p in [&a, &own] {
            assert!(!p.exists());
            assert_same_error(p);
        }
        // Nothing replaced the links.
        assert!(
            std::fs::symlink_metadata(&a)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn a_file_used_as_a_directory_is_an_error() {
        let s = Scratch::new("notdir");
        let f = s.file("plain.txt", b"x");
        assert_same_error(&f.join("child.txt"));
    }

    #[test]
    fn a_dangling_symlink_is_missing() {
        // The OS reports `NotFound` for the target, so this is a new file.
        let s = Scratch::new("dangling");
        let p = s.0.join("link.wrl");
        symlink(s.0.join("target.wrl"), &p).unwrap();
        assert_eq!(StdFs.read(&p).unwrap(), None);
    }
}

#[cfg(not(unix))]
#[test]
fn unix_permission_and_symlink_cases() {
    cm_evidence::tool_absent("Unix file modes and symlinks (this target is not Unix)")
}

// ---- App: error propagation through a controlled host ----

/// A host whose reads fail with `kind` (after `ok_reads` successful reads of `bytes`)
/// and whose writes fail when `fail_writes` is set.
struct Failing {
    kind: ErrorKind,
    ok_reads: usize,
    bytes: Vec<u8>,
    fail_writes: bool,
    written: Vec<Vec<u8>>,
}

impl Failing {
    fn reads(kind: ErrorKind) -> Failing {
        Failing {
            kind,
            ok_reads: 0,
            bytes: Vec::new(),
            fail_writes: false,
            written: Vec::new(),
        }
    }
}

impl FileHost for Failing {
    fn read(&mut self, _path: &Path) -> std::io::Result<Option<Vec<u8>>> {
        if self.ok_reads > 0 {
            self.ok_reads -= 1;
            return Ok(Some(self.bytes.clone()));
        }
        Err(std::io::Error::new(self.kind, "host read failed"))
    }
    fn write(&mut self, _path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        if self.fail_writes {
            return Err(std::io::Error::new(self.kind, "host write failed"));
        }
        self.written.push(bytes.to_vec());
        Ok(())
    }
}

#[test]
fn open_with_returns_the_host_read_error() {
    for kind in [
        ErrorKind::PermissionDenied,
        ErrorKind::InvalidInput,
        ErrorKind::Unsupported,
        ErrorKind::Interrupted,
        ErrorKind::Other,
    ] {
        let e = open(Box::new(Failing::reads(kind)), Path::new("/v/a.txt"))
            .err()
            .expect("no editor for a failed read");
        assert_eq!(e.kind(), kind);
        assert_eq!(e.to_string(), "host read failed");
    }
}

#[test]
fn a_failed_open_leaves_an_open_dirty_document_alone() {
    let path = Path::new("/v/doc.txt");
    let mut files = MemFiles::default();
    files.0.insert(path.to_path_buf(), b"kept\n".to_vec());
    let mut app = open(Box::new(files), path).unwrap();
    app.editor.doc.set_cursor(cm_core::Pos::new(1, 0), None);
    app.editor.type_text("edit");
    assert!(!app.editor.doc.is_clean(None));

    // Opening another file fails; the open document, its dirty state and its file
    // info are unchanged.
    let e = open(
        Box::new(Failing::reads(ErrorKind::PermissionDenied)),
        Path::new("/v/other.txt"),
    )
    .err()
    .unwrap();
    assert_eq!(e.kind(), ErrorKind::PermissionDenied);
    assert_eq!(app.editor.get_value(), "kept\nedit");
    assert!(!app.editor.doc.is_clean(None));
    assert_eq!(app.path().map(|p| p.as_path()), Some(path));
    app.save().unwrap();
    assert_eq!(
        app.files.read(path).unwrap().as_deref(),
        Some(&b"kept\nedit"[..])
    );
}

#[test]
fn a_read_failing_after_construction_does_not_reach_the_document() {
    let path = Path::new("/v/later.txt");
    let host = Failing {
        ok_reads: 1,
        bytes: b"first\r\n".to_vec(),
        ..Failing::reads(ErrorKind::PermissionDenied)
    };
    let mut app = open(Box::new(host), path).unwrap();
    assert_eq!(app.editor.get_value(), "first\r\n");
    app.editor.type_text("x");
    let e = app.files.read(path).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::PermissionDenied);
    assert_eq!(app.editor.get_value(), "xfirst\r\n");
    assert!(!app.editor.doc.is_clean(None));
    assert_eq!(
        app.file.as_ref().unwrap().line_separator.as_deref(),
        Some("\r\n")
    );
}

#[test]
fn a_failed_save_keeps_the_document_dirty() {
    let path = Path::new("/v/save.txt");
    let host = Failing {
        ok_reads: 1,
        bytes: b"a\n".to_vec(),
        fail_writes: true,
        ..Failing::reads(ErrorKind::PermissionDenied)
    };
    let mut app = open(Box::new(host), path).unwrap();
    app.editor.type_text("b");
    let e = app.save().unwrap_err();
    assert_eq!(e.kind(), ErrorKind::PermissionDenied);
    assert!(!app.editor.doc.is_clean(None));
    assert_eq!(app.editor.get_value(), "ba\n");
}

// ---- MemFiles ----

#[test]
fn mem_files_contract() {
    let mut m = MemFiles::default();
    let p = Path::new("/m/x.txt");
    assert_eq!(m.read(p).unwrap(), None);
    m.write(p, b"one").unwrap();
    assert_eq!(m.read(p).unwrap().as_deref(), Some(&b"one"[..]));
    m.write(p, b"").unwrap();
    assert_eq!(m.read(p).unwrap().as_deref(), Some(&b""[..]));
    assert!(!Path::new("/m").exists(), "no filesystem access");
}

// ---- encodings through StdFs and App ----

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(bytes).unwrap();
    e.finish().unwrap()
}

/// File name, bytes, document text, encoding, line separator, gzip.
type Case = (
    &'static str,
    Vec<u8>,
    &'static str,
    Encoding,
    Option<&'static str>,
    bool,
);

#[test]
fn encodings_round_trip_through_app_and_std_fs() {
    let s = Scratch::new("encodings");
    let cases: Vec<Case> = vec![
        (
            "utf8.txt",
            b"plain\n".to_vec(),
            "plain\n",
            Encoding::Utf8,
            None,
            false,
        ),
        (
            "unicode.txt",
            "\u{e9}\u{4e2d}\u{1f600}\n".as_bytes().to_vec(),
            "\u{e9}\u{4e2d}\u{1f600}\n",
            Encoding::Utf8,
            None,
            false,
        ),
        (
            "latin1.txt",
            b"caf\xe9\n".to_vec(),
            "caf\u{e9}\n",
            Encoding::Latin1,
            None,
            false,
        ),
        (
            "crlf.wrl",
            b"a\r\nb\r\n".to_vec(),
            "a\r\nb\r\n",
            Encoding::Utf8,
            Some("\r\n"),
            false,
        ),
        ("empty.txt", Vec::new(), "", Encoding::Utf8, None, false),
        (
            "gz.wrl",
            gzip(b"#VRML V2.0 utf8\n"),
            "#VRML V2.0 utf8\n",
            Encoding::Utf8,
            None,
            true,
        ),
    ];
    for (name, bytes, text, enc, sep, gz) in cases {
        let p = s.file(name, &bytes);
        let mut app = open_std(&p).unwrap();
        // `get_value` joins with the document's lineSeparator.
        assert_eq!(app.editor.get_value(), text, "{name}");
        let f = app.file.clone().unwrap();
        assert_eq!(
            (f.encoding, f.line_separator.as_deref(), f.gzip),
            (enc, sep, gz),
            "{name}"
        );
        // Saving an unedited document writes the same bytes (gzip: the same text).
        app.save().unwrap();
        let saved = std::fs::read(&p).unwrap();
        if gz {
            assert_eq!(fileio::decode(&p, &saved).unwrap().0, text, "{name}");
        } else {
            assert_eq!(saved, bytes, "{name}");
        }
        assert!(
            std::fs::read_dir(&s.0).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("cm-edit-tmp")),
            "no temporary file left"
        );
    }
}

#[test]
fn new_file_is_created_on_save() {
    let s = Scratch::new("new-file");
    let p = s.0.join("new.wrl");
    let mut app = open_std(&p).unwrap();
    app.editor.type_text("#VRML V2.0 utf8");
    app.save().unwrap();
    assert!(app.editor.doc.is_clean(None));
    assert_eq!(std::fs::read(&p).unwrap(), b"#VRML V2.0 utf8");
    let names: Vec<String> = std::fs::read_dir(&s.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["new.wrl"]);
}

#[test]
fn save_after_open_writes_the_edit() {
    let s = Scratch::new("save-edit");
    let p = s.file("e.wrl", b"a\r\nb\r\n");
    let mut app = open_std(&p).unwrap();
    app.editor.doc.set_cursor(cm_core::Pos::new(2, 0), None);
    app.editor.type_text("c");
    app.save().unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), b"a\r\nb\r\nc");
}

#[cfg(unix)]
#[test]
fn a_failed_std_fs_save_keeps_the_file_and_the_dirty_state() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("save-fail");
    let dir = s.0.join("ro");
    std::fs::create_dir(&dir).unwrap();
    let p = dir.join("f.txt");
    std::fs::write(&p, b"orig").unwrap();
    let mut app = open_std(&p).unwrap();
    app.editor.type_text("new ");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    if std::fs::File::create(dir.join("probe")).is_ok() {
        return cm_evidence::tool_absent("an unprivileged process (mode 555 was writable)");
    }
    let e = app.save().unwrap_err();
    assert_eq!(e.kind(), ErrorKind::PermissionDenied);
    assert!(!app.editor.doc.is_clean(None));
    assert_eq!(std::fs::read(&p).unwrap(), b"orig");
}
