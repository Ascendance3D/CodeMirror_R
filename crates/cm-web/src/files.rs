//! A `FileHost` over two host functions. Browser file access (an `<input
//! type=file>`, the File System Access API, `fetch`, a Tauri command) is
//! asynchronous, so the host reads the bytes first and hands the editor a
//! synchronous `read(path)`; `write(path, bytes)` hands the encoded bytes back.
//! Decoding and encoding (gzip, Latin-1, line separators) stay in
//! `cm_view::fileio`.

use std::path::Path;

use cm_view::fileio::FileHost;
use js_sys::{Function, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};

/// `read(path) -> Uint8Array | null | undefined` (absent: a new file) and
/// `write(path, Uint8Array)`. A function that throws is a failed read or write.
pub struct JsFiles {
    read: Option<Function>,
    write: Option<Function>,
}

fn io_error(what: &str, e: JsValue) -> std::io::Error {
    let msg = e
        .dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| e.as_string())
        .unwrap_or_else(|| format!("{e:?}"));
    std::io::Error::other(format!("{what}: {msg}"))
}

impl JsFiles {
    /// From an object with optional `read` and `write` functions.
    pub fn from_object(o: &JsValue) -> JsFiles {
        let f = |name: &str| {
            Reflect::get(o, &JsValue::from_str(name))
                .ok()
                .and_then(|v| v.dyn_into::<Function>().ok())
        };
        JsFiles {
            read: f("read"),
            write: f("write"),
        }
    }
}

impl FileHost for JsFiles {
    fn read(&mut self, path: &Path) -> std::io::Result<Option<Vec<u8>>> {
        let Some(read) = &self.read else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "no read function",
            ));
        };
        let p = JsValue::from_str(&path.to_string_lossy());
        let v = read
            .call1(&JsValue::NULL, &p)
            .map_err(|e| io_error("read failed", e))?;
        if v.is_null() || v.is_undefined() {
            return Ok(None);
        }
        let bytes = v
            .dyn_into::<Uint8Array>()
            .map_err(|_| std::io::Error::other("read failed: not a Uint8Array"))?;
        Ok(Some(bytes.to_vec()))
    }

    fn write(&mut self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let Some(write) = &self.write else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "no write function",
            ));
        };
        let p = JsValue::from_str(&path.to_string_lossy());
        write
            .call2(&JsValue::NULL, &p, &Uint8Array::from(bytes))
            .map_err(|e| io_error("write failed", e))?;
        Ok(())
    }
}
