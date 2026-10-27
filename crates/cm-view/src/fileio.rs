//! Opening and saving files without changing bytes the user did not edit.
//!
//! * gzip-compressed files (most historical `.wrl` files are gzip under a `.wrl`
//!   name) are decompressed in memory and written back compressed;
//! * text that is not valid UTF-8 is read as Latin-1 (one char per byte) and
//!   written back the same way;
//! * a file whose line breaks are all `\r\n` (or all `\r`) gets that as the
//!   document's `lineSeparator`, so saving keeps it; mixed line breaks are
//!   normalized to `\n` on save (CM5's `getValue` behavior), which [`FileInfo::mixed_newlines`]
//!   reports.
//!
//! [`decode`] and [`encode`] are pure and run on every target. Reading and writing
//! the bytes is a host service ([`FileHost`]): [`StdFs`] is the local filesystem
//! (native targets), [`MemFiles`] keeps files in memory (tests, or a host that moves
//! the bytes itself, such as a browser).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Latin1,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileInfo {
    pub path: PathBuf,
    pub gzip: bool,
    pub encoding: Encoding,
    /// `lineSeparator` to use (`None`: split on any break, save with `\n`).
    pub line_separator: Option<String>,
    pub mixed_newlines: bool,
}

/// Decode file bytes into text plus how to write it back.
pub fn decode(path: &Path, bytes: &[u8]) -> std::io::Result<(String, FileInfo)> {
    let (raw, gzip) = if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        let mut d = flate2::read::MultiGzDecoder::new(bytes);
        let mut out = Vec::new();
        d.read_to_end(&mut out)?;
        (out, true)
    } else {
        (bytes.to_vec(), false)
    };
    let (text, encoding) = match String::from_utf8(raw) {
        Ok(s) => (s, Encoding::Utf8),
        Err(e) => (
            e.into_bytes().iter().map(|&b| b as char).collect(),
            Encoding::Latin1,
        ),
    };
    let crlf = text.matches("\r\n").count();
    let cr = text.matches('\r').count() - crlf;
    let lf = text.matches('\n').count() - crlf;
    let kinds = [crlf > 0, cr > 0, lf > 0].iter().filter(|&&k| k).count();
    let line_separator = if kinds == 1 && crlf > 0 {
        Some("\r\n".to_string())
    } else if kinds == 1 && cr > 0 {
        Some("\r".to_string())
    } else {
        None
    };
    Ok((
        text,
        FileInfo {
            path: path.to_path_buf(),
            gzip,
            encoding,
            line_separator,
            mixed_newlines: kinds > 1,
        },
    ))
}

/// Read a file from the local filesystem.
pub fn load(path: &Path) -> std::io::Result<(String, FileInfo)> {
    let bytes = std::fs::read(path)?;
    decode(path, &bytes)
}

/// Where file bytes come from and go to. Editing a document never needs one.
pub trait FileHost {
    /// The file's bytes, or `None` when the file does not exist (a new file).
    /// Any other failure (permission, a symlink loop, an invalid path, no
    /// filesystem) is an `Err`, never `None`: [`App::open_with`](crate::app::App::open_with)
    /// opens an empty document only for `None`.
    fn read(&mut self, path: &Path) -> std::io::Result<Option<Vec<u8>>>;
    /// Replace the file's bytes.
    fn write(&mut self, path: &Path, bytes: &[u8]) -> std::io::Result<()>;
}

/// The local filesystem through `std::fs`. A file is missing only when the OS
/// reports `NotFound`; other read errors are returned as they are. On
/// `wasm32-unknown-unknown` every call returns an `Unsupported` error.
#[derive(Clone, Copy, Debug, Default)]
pub struct StdFs;

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl FileHost for StdFs {
    fn read(&mut self, path: &Path) -> std::io::Result<Option<Vec<u8>>> {
        // One read, no `Path::exists` first: `exists` is false for every metadata
        // error, which would make an unreadable file look new.
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
    fn write(&mut self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        write_replacing(path, bytes)
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl FileHost for StdFs {
    fn read(&mut self, _path: &Path) -> std::io::Result<Option<Vec<u8>>> {
        Err(no_local_filesystem())
    }
    fn write(&mut self, _path: &Path, _bytes: &[u8]) -> std::io::Result<()> {
        Err(no_local_filesystem())
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn no_local_filesystem() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "StdFs: no local filesystem on wasm32-unknown-unknown (use a host FileHost)",
    )
}

/// Files kept in memory, by path.
#[derive(Clone, Debug, Default)]
pub struct MemFiles(pub HashMap<PathBuf, Vec<u8>>);

impl FileHost for MemFiles {
    fn read(&mut self, path: &Path) -> std::io::Result<Option<Vec<u8>>> {
        Ok(self.0.get(path).cloned())
    }
    fn write(&mut self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        self.0.insert(path.to_path_buf(), bytes.to_vec());
        Ok(())
    }
}

/// Encode text for writing.
pub fn encode(text: &str, info: &FileInfo) -> std::io::Result<Vec<u8>> {
    let raw: Vec<u8> = match info.encoding {
        Encoding::Utf8 => text.as_bytes().to_vec(),
        Encoding::Latin1 => {
            let mut v = Vec::with_capacity(text.len());
            for c in text.chars() {
                let u = c as u32;
                if u > 0xff {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("character U+{u:04X} cannot be saved in this Latin-1 file"),
                    ));
                }
                v.push(u as u8);
            }
            v
        }
    };
    if info.gzip {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(&raw)?;
        e.finish()
    } else {
        Ok(raw)
    }
}

/// Write a file to the local filesystem (to a temporary sibling, then rename).
pub fn save(path: &Path, text: &str, info: &FileInfo) -> std::io::Result<()> {
    let bytes = encode(text, info)?;
    write_replacing(path, &bytes)
}

fn write_replacing(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!(
        "{}.cm-edit-tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips() {
        let p = Path::new("/tmp/x.wrl");
        for bytes in [
            &b"#VRML V2.0 utf8\r\nShape {}\r\n"[..],
            b"a\nb\n",
            b"caf\xe9\n",
            b"x\ry\r",
        ] {
            let (t, info) = decode(p, bytes).unwrap();
            let lines: Vec<&str> = t.split(['\r', '\n']).collect();
            assert!(!lines.is_empty());
            assert_eq!(encode(&t, &info).unwrap(), bytes);
        }
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(b"#VRML V2.0 utf8\n").unwrap();
        let gz = gz.finish().unwrap();
        let (t, info) = decode(p, &gz).unwrap();
        assert!(info.gzip);
        assert_eq!(t, "#VRML V2.0 utf8\n");
    }
}
