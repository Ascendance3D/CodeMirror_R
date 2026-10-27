//! The editor application's input layer, independent of the windowing toolkit:
//! key/text/mouse/wheel/resize events in, editor state and file saves out. The
//! winit front end ([`crate::window`]) and the scripted tests both drive this.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use std::rc::Rc;

use cm_modes::Spec;

use crate::editor::{Editor, EditorOptions, mode_for_path};
use crate::fileio::{self, FileHost, FileInfo, StdFs};
use crate::keymap::KeyEvent;
use crate::measure::Metrics;
use crate::mouse::MouseEvent;

/// The system clipboard, or a stand-in.
pub trait Clipboard {
    fn get(&mut self) -> Option<String>;
    fn set(&mut self, text: &str);
}

/// An in-memory clipboard (tests, or when no system clipboard is available).
#[derive(Default, Debug)]
pub struct MemClipboard(pub Option<String>);

impl Clipboard for MemClipboard {
    fn get(&mut self) -> Option<String> {
        self.0.clone()
    }
    fn set(&mut self, text: &str) {
        self.0 = Some(text.to_string());
    }
}

/// One input event, in window pixels where positions apply.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    /// A key press; `text` is what the key types, if anything.
    Key {
        key: KeyEvent,
        text: Option<String>,
    },
    /// Committed text without a key (IME).
    Text(String),
    MouseDown {
        x: f64,
        y: f64,
        button: u8,
        mods: KeyEvent,
        time: f64,
    },
    MouseMove {
        x: f64,
        y: f64,
        mods: KeyEvent,
    },
    MouseUp {
        x: f64,
        y: f64,
    },
    /// Scroll by pixels.
    Wheel {
        dx: f64,
        dy: f64,
    },
    Resize {
        width: f64,
        height: f64,
    },
    /// Run a named command.
    Command(String),
    Save,
}

/// Application options.
#[derive(Clone, Debug, Default)]
pub struct AppOptions {
    pub read_only: bool,
    pub line_wrapping: bool,
    pub no_line_numbers: bool,
    /// Override the mode (a MIME type or mode name).
    pub mode: Option<String>,
}

pub struct App {
    pub editor: Editor,
    pub file: Option<FileInfo>,
    pub clipboard: Box<dyn Clipboard>,
    /// Where [`App::save`] writes (the local filesystem unless the host replaces it).
    pub files: Box<dyn FileHost>,
    /// Status line text (last error or message).
    pub status: String,
    save_requested: Rc<Cell<bool>>,
}

impl App {
    /// An editor for `text` with the file info `file` (or a scratch buffer).
    pub fn new(
        text: &str,
        file: Option<FileInfo>,
        metrics: Metrics,
        opts: &AppOptions,
        clipboard: Box<dyn Clipboard>,
    ) -> App {
        let mode = match (&opts.mode, &file) {
            (Some(m), _) => Spec::from(m.as_str()),
            (None, Some(f)) => mode_for_path(&f.path.to_string_lossy()),
            (None, None) => Spec::from("text/plain"),
        };
        let options = EditorOptions {
            read_only: opts.read_only,
            line_wrapping: opts.line_wrapping,
            line_numbers: !opts.no_line_numbers,
            line_separator: file.as_ref().and_then(|f| f.line_separator.clone()),
            ..Default::default()
        };
        let mut editor = Editor::new(text, mode, options, metrics);
        editor.doc.clear_history();
        editor.doc.mark_clean();
        let save_requested = Rc::new(Cell::new(false));
        let flag = save_requested.clone();
        // `CodeMirror.commands.save`, which the default key maps bind to Ctrl-S/Cmd-S.
        editor.commands.insert(
            "save".into(),
            Rc::new(move |_cm: &mut Editor| {
                flag.set(true);
                true
            }),
        );
        let status = match &file {
            Some(f) if f.mixed_newlines => "mixed line endings: saving writes \\n".to_string(),
            _ => String::new(),
        };
        App {
            editor,
            file,
            clipboard,
            files: Box::new(StdFs),
            status,
            save_requested,
        }
    }

    /// Open a file from the local filesystem (a missing file opens empty and is
    /// created on save).
    pub fn open(
        path: &Path,
        metrics: Metrics,
        opts: &AppOptions,
        clipboard: Box<dyn Clipboard>,
    ) -> std::io::Result<App> {
        App::open_with(Box::new(StdFs), path, metrics, opts, clipboard)
    }

    /// Open a file through `files`, which [`App::save`] then writes to.
    pub fn open_with(
        mut files: Box<dyn FileHost>,
        path: &Path,
        metrics: Metrics,
        opts: &AppOptions,
        clipboard: Box<dyn Clipboard>,
    ) -> std::io::Result<App> {
        let (text, info) = if let Some(bytes) = files.read(path)? {
            fileio::decode(path, &bytes)?
        } else {
            (
                String::new(),
                FileInfo {
                    path: path.to_path_buf(),
                    gzip: false,
                    encoding: fileio::Encoding::Utf8,
                    line_separator: None,
                    mixed_newlines: false,
                },
            )
        };
        let mut app = App::new(&text, Some(info), metrics, opts, clipboard);
        app.files = files;
        Ok(app)
    }

    pub fn path(&self) -> Option<&PathBuf> {
        self.file.as_ref().map(|f| &f.path)
    }

    /// Window title: file name, a dot when modified, `[read-only]`.
    pub fn title(&self) -> String {
        let name = self
            .path()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".into());
        let dirty = if self.editor.doc.is_clean(None) {
            ""
        } else {
            " \u{2022}"
        };
        let ro = if self.editor.options.read_only {
            " [read-only]"
        } else {
            ""
        };
        format!("{name}{dirty}{ro} \u{2014} cm-edit")
    }

    /// Save to the file the buffer came from.
    pub fn save(&mut self) -> std::io::Result<()> {
        let Some(info) = self.file.clone() else {
            return Err(std::io::Error::other("no file name"));
        };
        if self.editor.options.read_only {
            return Err(std::io::Error::other("read-only"));
        }
        let text = self.editor.get_value();
        let bytes = fileio::encode(&text, &info)?;
        self.files.write(&info.path, &bytes)?;
        self.editor.doc.mark_clean();
        self.status = format!("saved {}", info.path.display());
        Ok(())
    }

    fn flush_save(&mut self) {
        if self.save_requested.replace(false) {
            if let Err(e) = self.save() {
                self.status = format!("save failed: {e}");
            }
        }
    }

    fn mouse_event(&self, x: f64, y: f64, button: u8, mods: &KeyEvent, time: f64) -> MouseEvent {
        let (dx, dy) = self.editor.div_from_window(x, y);
        MouseEvent {
            x: if x < self.editor.gutter_width {
                -1.0
            } else {
                dx
            },
            y: dy,
            button,
            shift: mods.shift,
            ctrl: mods.ctrl,
            alt: mods.alt,
            meta: mods.meta,
            time,
        }
    }

    /// Handle one input event. Returns whether the view needs a redraw.
    pub fn handle(&mut self, input: Input) -> bool {
        let r = self.handle_input(input);
        // Like `setTimeout(fireOrphanDelayed, 0)` after the browser event.
        self.deliver_pending();
        r
    }

    fn handle_input(&mut self, input: Input) -> bool {
        match input {
            Input::Key { key, text } => {
                let handled = self.editor.key_down(&key);
                if !handled {
                    let primary = if self.editor.options.mac {
                        key.meta
                    } else {
                        key.ctrl
                    };
                    let k = key.key.as_str();
                    if (primary && k == "C") || (key.ctrl && k == "Insert") {
                        if let Some(t) = self.editor.copy() {
                            self.clipboard.set(&t);
                        }
                    } else if (primary && k == "X") || (key.shift && k == "Delete") {
                        if let Some(t) = self.editor.cut() {
                            self.clipboard.set(&t);
                        }
                    } else if (primary && k == "V") || (key.shift && k == "Insert") {
                        if let Some(t) = self.clipboard.get() {
                            self.editor.paste(&t);
                        }
                    } else if let Some(t) = text {
                        if !t.is_empty() && !t.chars().all(|c| c.is_control()) {
                            self.editor.key_press(&t, &key);
                        }
                    }
                }
                self.flush_save();
                true
            }
            Input::Text(t) => {
                self.editor.type_text(&t);
                true
            }
            Input::MouseDown {
                x,
                y,
                button,
                mods,
                time,
            } => {
                let e = self.mouse_event(x, y, button, &mods, time);
                if e.x < 0.0 {
                    return false;
                }
                self.editor.mouse_down(&e);
                true
            }
            Input::MouseMove { x, y, mods } => {
                if !self.editor.mouse_active() {
                    return false;
                }
                let e = self.mouse_event(x.max(self.editor.gutter_width), y, 1, &mods, 0.0);
                self.editor.mouse_move(&e);
                // Dragging past the top/bottom scrolls (`extend` in leftButtonSelect).
                if y < 0.0 {
                    let st = self.editor.display.scroll_top;
                    self.editor
                        .scroll_to(None, Some(st - self.editor.metrics.text_height));
                } else if y > self.editor.display.height {
                    let st = self.editor.display.scroll_top;
                    self.editor
                        .scroll_to(None, Some(st + self.editor.metrics.text_height));
                }
                true
            }
            Input::MouseUp { x, y } => {
                let e = self.mouse_event(
                    x.max(self.editor.gutter_width),
                    y,
                    1,
                    &KeyEvent::default(),
                    0.0,
                );
                self.editor.mouse_up(Some(&e));
                true
            }
            Input::Wheel { dx, dy } => {
                let (st, sl) = (
                    self.editor.display.scroll_top,
                    self.editor.display.scroll_left,
                );
                self.editor.scroll_to(Some(sl + dx), Some(st + dy));
                true
            }
            Input::Resize { width, height } => {
                self.editor.set_size(width, height);
                true
            }
            Input::Command(c) => {
                self.editor.exec_command(&c);
                self.flush_save();
                true
            }
            Input::Save => {
                if let Err(e) = self.save() {
                    self.status = format!("save failed: {e}");
                }
                true
            }
        }
    }

    /// Run the background highlighter for a bounded amount of work; returns
    /// whether more remains.
    pub fn idle(&mut self) -> bool {
        self.deliver_pending();
        self.editor.highlight_step(200)
    }

    /// Deliver event callbacks queued outside an operation (the orphan queue) and
    /// editor work parked by direct document calls.
    pub fn deliver_pending(&mut self) {
        if cm_core::operation_group::orphans_pending() || self.editor.parked_len() > 0 {
            self.editor.fire_orphan_delayed();
        }
    }

    /// The window gained or lost focus (`focus`/`blur` events).
    pub fn set_focused(&mut self, focused: bool) {
        self.editor.set_focused(focused);
    }
}

/// A scripted step (`--script`): `key <Name>` / `type <text>` / `click <x> <y>` /
/// `shiftclick <x> <y>` / `dblclick <x> <y>` / `drag <x0> <y0> <x1> <y1>` /
/// `command <name>` / `wheel <dy>` / `pause <ms>` / `save` / `screenshot <png>` / `quit`.
#[derive(Clone, Debug)]
pub enum Step {
    Input(Vec<Input>),
    /// `pause <ms>`: let time pass (history events stop merging after 1250ms).
    /// A scripted run uses a manual clock that the pause advances, so the result
    /// does not depend on how fast the machine replays the steps.
    Pause(u64),
    Screenshot(PathBuf),
    Quit,
}

pub fn parse_script(src: &str) -> Result<Vec<Step>, String> {
    let mut out = Vec::new();
    let mut t = 1000.0;
    for (n, line) in src.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        let nums = |k: usize| -> Result<Vec<f64>, String> {
            let v: Vec<f64> = rest
                .split_whitespace()
                .map(|s| s.parse::<f64>().map_err(|e| format!("line {}: {e}", n + 1)))
                .collect::<Result<_, _>>()?;
            if v.len() != k {
                return Err(format!("line {}: expected {k} numbers", n + 1));
            }
            Ok(v)
        };
        t += 1000.0;
        let none = KeyEvent::default();
        let step = match cmd {
            "key" => {
                let k = KeyEvent::parse(rest.trim());
                let text = if !k.ctrl && !k.alt && !k.meta && k.key.chars().count() == 1 {
                    let c = k.key.chars().next().unwrap();
                    Some(if k.shift {
                        c.to_string()
                    } else {
                        c.to_ascii_lowercase().to_string()
                    })
                } else if k.key == "Space" {
                    Some(" ".into())
                } else {
                    None
                };
                Step::Input(vec![Input::Key { key: k, text }])
            }
            "type" => {
                // One key event per character, as typing does.
                let unescaped = rest.replace("\\n", "\n").replace("\\t", "\t");
                Step::Input(
                    unescaped
                        .chars()
                        .map(|c| match c {
                            '\n' => Input::Key {
                                key: KeyEvent::new("Enter"),
                                text: Some("\r".into()),
                            },
                            '\t' => Input::Key {
                                key: KeyEvent::new("Tab"),
                                text: Some("\t".into()),
                            },
                            c => Input::Key {
                                key: KeyEvent {
                                    key: String::new(),
                                    ..Default::default()
                                },
                                text: Some(c.to_string()),
                            },
                        })
                        .collect(),
                )
            }
            "click" | "shiftclick" | "dblclick" => {
                let v = nums(2)?;
                let mods = KeyEvent {
                    shift: cmd == "shiftclick",
                    ..Default::default()
                };
                let mut evs = vec![
                    Input::MouseDown {
                        x: v[0],
                        y: v[1],
                        button: 1,
                        mods: mods.clone(),
                        time: t,
                    },
                    Input::MouseUp { x: v[0], y: v[1] },
                ];
                if cmd == "dblclick" {
                    evs.push(Input::MouseDown {
                        x: v[0],
                        y: v[1],
                        button: 1,
                        mods,
                        time: t + 100.0,
                    });
                    evs.push(Input::MouseUp { x: v[0], y: v[1] });
                }
                Step::Input(evs)
            }
            "drag" => {
                let v = nums(4)?;
                Step::Input(vec![
                    Input::MouseDown {
                        x: v[0],
                        y: v[1],
                        button: 1,
                        mods: none.clone(),
                        time: t,
                    },
                    Input::MouseMove {
                        x: (v[0] + v[2]) / 2.0,
                        y: (v[1] + v[3]) / 2.0,
                        mods: none.clone(),
                    },
                    Input::MouseMove {
                        x: v[2],
                        y: v[3],
                        mods: none,
                    },
                    Input::MouseUp { x: v[2], y: v[3] },
                ])
            }
            "command" => Step::Input(vec![Input::Command(rest.trim().to_string())]),
            "save" => Step::Input(vec![Input::Save]),
            "wheel" => {
                let v = nums(1)?;
                Step::Input(vec![Input::Wheel { dx: 0.0, dy: v[0] }])
            }
            "screenshot" => Step::Screenshot(PathBuf::from(rest.trim())),
            "pause" => Step::Pause(
                rest.trim()
                    .parse()
                    .map_err(|e| format!("line {}: {e}", n + 1))?,
            ),
            "quit" => Step::Quit,
            _ => return Err(format!("line {}: unknown step {cmd:?}", n + 1)),
        };
        out.push(step);
    }
    Ok(out)
}
