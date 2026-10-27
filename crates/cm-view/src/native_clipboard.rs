//! The system clipboard, for hosts that have no clipboard of their own.
//!
//! With the `clipboard-native` feature on a desktop target that `arboard` supports
//! (Linux and the BSDs, Windows, macOS), [`SystemClipboard`] uses `arboard`.
//! Everywhere else (WebAssembly, Android, iOS, or the feature off) it is an
//! in-memory clipboard; a browser or mobile host passes its own [`Clipboard`].

use crate::app::{Clipboard, MemClipboard};

#[cfg(all(
    feature = "clipboard-native",
    any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "windows",
        target_os = "macos"
    )
))]
mod sys {
    pub struct Sys(arboard::Clipboard);
    pub fn open() -> Option<Sys> {
        arboard::Clipboard::new().ok().map(Sys)
    }
    impl Sys {
        pub fn get(&mut self) -> Option<String> {
            self.0.get_text().ok()
        }
        pub fn set(&mut self, text: &str) {
            let _ = self.0.set_text(text.to_string());
        }
    }
}

#[cfg(not(all(
    feature = "clipboard-native",
    any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "windows",
        target_os = "macos"
    )
)))]
mod sys {
    pub enum Sys {}
    pub fn open() -> Option<Sys> {
        None
    }
    impl Sys {
        pub fn get(&mut self) -> Option<String> {
            match *self {}
        }
        pub fn set(&mut self, _text: &str) {
            match *self {}
        }
    }
}

/// The system clipboard when there is one, else an in-memory clipboard.
pub struct SystemClipboard {
    sys: Option<sys::Sys>,
    mem: MemClipboard,
}

impl SystemClipboard {
    /// Open the system clipboard; falls back to memory when it cannot be opened.
    pub fn new() -> SystemClipboard {
        SystemClipboard {
            sys: sys::open(),
            mem: MemClipboard::default(),
        }
    }

    /// An in-memory clipboard (scripted runs).
    pub fn in_memory() -> SystemClipboard {
        SystemClipboard {
            sys: None,
            mem: MemClipboard::default(),
        }
    }

    /// Whether the system clipboard is in use.
    pub fn is_system(&self) -> bool {
        self.sys.is_some()
    }
}

impl Default for SystemClipboard {
    fn default() -> Self {
        SystemClipboard::new()
    }
}

impl Clipboard for SystemClipboard {
    fn get(&mut self) -> Option<String> {
        match &mut self.sys {
            Some(c) => c.get(),
            None => self.mem.get(),
        }
    }
    fn set(&mut self, text: &str) {
        match &mut self.sys {
            Some(c) => c.set(text),
            None => self.mem.set(text),
        }
    }
}
