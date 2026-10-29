//! `src/input/keymap.js` and `src/input/keynames.js`: key maps, key name
//! normalization and lookup with fallthrough.

use std::collections::HashMap;
use std::rc::Rc;

use crate::editor::Editor;

/// A handler function bound in a key map (`keymap["X"] = function(cm) {...}`).
/// Returning `false` is `CodeMirror.Pass`.
pub type KeyFn = Rc<dyn Fn(&mut Editor) -> bool>;
/// A [`Binding::Mouse`] function; returning `false` is `CodeMirror.Pass`.
pub type MouseFn = Rc<dyn Fn(&mut Editor, Option<cm_core::Pos>) -> bool>;

/// A key map value.
#[derive(Clone)]
pub enum Binding {
    /// A command name.
    Command(String),
    /// A function.
    Fn(KeyFn),
    /// A function that takes the click position: what a mouse binding
    /// (`"Shift-LeftClick"`, mouse_events.js:96) is called with; `None` when a key
    /// runs it (the JS `pos` is then undefined).
    Mouse(MouseFn),
    /// `false`: stop lookup, do nothing ("nothing").
    Nothing,
    /// `"..."`: the prefix of a multi-stroke binding.
    Multi,
}

impl std::fmt::Debug for Binding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Binding::Command(c) => write!(f, "Command({c:?})"),
            Binding::Fn(_) => write!(f, "Fn"),
            Binding::Mouse(_) => write!(f, "Mouse"),
            Binding::Nothing => write!(f, "Nothing"),
            Binding::Multi => write!(f, "Multi"),
        }
    }
}

/// A key map: bindings plus `fallthrough` (names of global maps, or maps).
#[derive(Clone, Debug, Default)]
pub struct KeyMap {
    pub name: Option<String>,
    pub bindings: HashMap<String, Binding>,
    pub fallthrough: Vec<KeyMapRef>,
}

/// `getKeyMap(val)`: a map, or the name of one in [`KeyMaps`].
#[derive(Clone, Debug)]
pub enum KeyMapRef {
    Named(String),
    Map(Rc<KeyMap>),
}

impl KeyMap {
    /// Build from `(key, command)` pairs, normalizing names (`normalizeKeyMap`).
    pub fn from_pairs(name: &str, pairs: &[(&str, &str)], fallthrough: &[&str]) -> KeyMap {
        let mut m = KeyMap {
            name: Some(name.to_string()),
            bindings: HashMap::new(),
            fallthrough: fallthrough
                .iter()
                .map(|f| KeyMapRef::Named(f.to_string()))
                .collect(),
        };
        for (k, v) in pairs {
            m.bind(k, Binding::Command(v.to_string()))
                .expect("consistent map");
        }
        m
    }

    /// Add one binding the way `normalizeKeyMap` does: normalize each stroke and
    /// register the prefixes of multi-stroke keys as `"..."`.
    pub fn bind(&mut self, keyname: &str, value: Binding) -> Result<(), String> {
        let keys: Vec<String> = keyname
            .split(' ')
            .map(normalize_key_name)
            .collect::<Result<_, _>>()?;
        for i in 0..keys.len() {
            let (name, val) = if i == keys.len() - 1 {
                (keys.join(" "), value.clone())
            } else {
                (keys[..=i].join(" "), Binding::Multi)
            };
            match self.bindings.get(&name) {
                None => {
                    self.bindings.insert(name, val);
                }
                Some(prev) => {
                    let same = match (prev, &val) {
                        (Binding::Command(a), Binding::Command(b)) => a == b,
                        (Binding::Multi, Binding::Multi) | (Binding::Nothing, Binding::Nothing) => {
                            true
                        }
                        _ => false,
                    };
                    if !same {
                        return Err(format!("Inconsistent bindings for {name}"));
                    }
                }
            }
        }
        Ok(())
    }
}

/// `normalizeKeyName(name)`: modifiers in the canonical order
/// `Shift-Cmd-Ctrl-Alt-`.
pub fn normalize_key_name(name: &str) -> Result<String, String> {
    // name.split(/-(?!$)/)
    let mut parts: Vec<&str> = Vec::new();
    let mut start = 0;
    let b = name.as_bytes();
    for i in 0..b.len() {
        if b[i] == b'-' && i + 1 < b.len() && i >= start {
            parts.push(&name[start..i]);
            start = i + 1;
        }
    }
    parts.push(&name[start..]);
    let base = parts[parts.len() - 1];
    let (mut alt, mut ctrl, mut shift, mut cmd) = (false, false, false, false);
    for m in &parts[..parts.len() - 1] {
        let l = m.to_ascii_lowercase();
        if matches!(l.as_str(), "cmd" | "meta" | "m") {
            cmd = true
        } else if matches!(l.as_str(), "a" | "alt") {
            alt = true
        } else if matches!(l.as_str(), "c" | "ctrl" | "control") {
            ctrl = true
        } else if matches!(l.as_str(), "s" | "shift") {
            shift = true
        } else {
            return Err(format!("Unrecognized modifier name: {m}"));
        }
    }
    let mut n = base.to_string();
    if alt {
        n = format!("Alt-{n}");
    }
    if ctrl {
        n = format!("Ctrl-{n}");
    }
    if cmd {
        n = format!("Cmd-{n}");
    }
    if shift {
        n = format!("Shift-{n}");
    }
    Ok(n)
}

/// The global `keyMap` object.
#[derive(Clone, Debug)]
pub struct KeyMaps {
    pub maps: HashMap<String, Rc<KeyMap>>,
}

impl KeyMaps {
    /// `keyMap.basic`, `pcDefault`, `emacsy`, `macDefault`, and `default` (pcDefault,
    /// or macDefault when `mac`).
    pub fn standard(mac: bool) -> KeyMaps {
        let basic = KeyMap::from_pairs(
            "basic",
            &[
                ("Left", "goCharLeft"),
                ("Right", "goCharRight"),
                ("Up", "goLineUp"),
                ("Down", "goLineDown"),
                ("End", "goLineEnd"),
                ("Home", "goLineStartSmart"),
                ("PageUp", "goPageUp"),
                ("PageDown", "goPageDown"),
                ("Delete", "delCharAfter"),
                ("Backspace", "delCharBefore"),
                ("Shift-Backspace", "delCharBefore"),
                ("Tab", "defaultTab"),
                ("Shift-Tab", "indentAuto"),
                ("Enter", "newlineAndIndent"),
                ("Insert", "toggleOverwrite"),
                ("Esc", "singleSelection"),
            ],
            &[],
        );
        let pc = KeyMap::from_pairs(
            "pcDefault",
            &[
                ("Ctrl-A", "selectAll"),
                ("Ctrl-D", "deleteLine"),
                ("Ctrl-Z", "undo"),
                ("Shift-Ctrl-Z", "redo"),
                ("Ctrl-Y", "redo"),
                ("Ctrl-Home", "goDocStart"),
                ("Ctrl-End", "goDocEnd"),
                ("Ctrl-Up", "goLineUp"),
                ("Ctrl-Down", "goLineDown"),
                ("Ctrl-Left", "goGroupLeft"),
                ("Ctrl-Right", "goGroupRight"),
                ("Alt-Left", "goLineStart"),
                ("Alt-Right", "goLineEnd"),
                ("Ctrl-Backspace", "delGroupBefore"),
                ("Ctrl-Delete", "delGroupAfter"),
                ("Ctrl-S", "save"),
                ("Ctrl-F", "find"),
                ("Ctrl-G", "findNext"),
                ("Shift-Ctrl-G", "findPrev"),
                ("Shift-Ctrl-F", "replace"),
                ("Shift-Ctrl-R", "replaceAll"),
                ("Ctrl-[", "indentLess"),
                ("Ctrl-]", "indentMore"),
                ("Ctrl-U", "undoSelection"),
                ("Shift-Ctrl-U", "redoSelection"),
                ("Alt-U", "redoSelection"),
            ],
            &["basic"],
        );
        let emacsy = KeyMap::from_pairs(
            "emacsy",
            &[
                ("Ctrl-F", "goCharRight"),
                ("Ctrl-B", "goCharLeft"),
                ("Ctrl-P", "goLineUp"),
                ("Ctrl-N", "goLineDown"),
                ("Ctrl-A", "goLineStart"),
                ("Ctrl-E", "goLineEnd"),
                ("Ctrl-V", "goPageDown"),
                ("Shift-Ctrl-V", "goPageUp"),
                ("Ctrl-D", "delCharAfter"),
                ("Ctrl-H", "delCharBefore"),
                ("Alt-Backspace", "delWordBefore"),
                ("Ctrl-K", "killLine"),
                ("Ctrl-T", "transposeChars"),
                ("Ctrl-O", "openLine"),
            ],
            &[],
        );
        let macd = KeyMap::from_pairs(
            "macDefault",
            &[
                ("Cmd-A", "selectAll"),
                ("Cmd-D", "deleteLine"),
                ("Cmd-Z", "undo"),
                ("Shift-Cmd-Z", "redo"),
                ("Cmd-Y", "redo"),
                ("Cmd-Home", "goDocStart"),
                ("Cmd-Up", "goDocStart"),
                ("Cmd-End", "goDocEnd"),
                ("Cmd-Down", "goDocEnd"),
                ("Alt-Left", "goGroupLeft"),
                ("Alt-Right", "goGroupRight"),
                ("Cmd-Left", "goLineLeft"),
                ("Cmd-Right", "goLineRight"),
                ("Alt-Backspace", "delGroupBefore"),
                ("Ctrl-Alt-Backspace", "delGroupAfter"),
                ("Alt-Delete", "delGroupAfter"),
                ("Cmd-S", "save"),
                ("Cmd-F", "find"),
                ("Cmd-G", "findNext"),
                ("Shift-Cmd-G", "findPrev"),
                ("Cmd-Alt-F", "replace"),
                ("Shift-Cmd-Alt-F", "replaceAll"),
                ("Cmd-[", "indentLess"),
                ("Cmd-]", "indentMore"),
                ("Cmd-Backspace", "delWrappedLineLeft"),
                ("Cmd-Delete", "delWrappedLineRight"),
                ("Cmd-U", "undoSelection"),
                ("Shift-Cmd-U", "redoSelection"),
                ("Ctrl-Up", "goDocStart"),
                ("Ctrl-Down", "goDocEnd"),
            ],
            &["basic", "emacsy"],
        );
        let mut maps = HashMap::new();
        let pc = Rc::new(pc);
        let macd = Rc::new(macd);
        maps.insert(
            "default".to_string(),
            if mac { macd.clone() } else { pc.clone() },
        );
        maps.insert("basic".to_string(), Rc::new(basic));
        maps.insert("pcDefault".to_string(), pc);
        maps.insert("emacsy".to_string(), Rc::new(emacsy));
        maps.insert("macDefault".to_string(), macd);
        KeyMaps { maps }
    }

    pub fn get(&self, r: &KeyMapRef) -> Option<Rc<KeyMap>> {
        match r {
            KeyMapRef::Named(n) => self.maps.get(n).cloned(),
            KeyMapRef::Map(m) => Some(m.clone()),
        }
    }
}

/// The result of [`lookup_key`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    Nothing,
    Multi,
    Handled,
}

/// `lookupKey(key, map, handle, context)`. `None` is the JS `undefined`.
pub fn lookup_key(
    maps: &KeyMaps,
    key: &str,
    map: &KeyMapRef,
    handle: &mut dyn FnMut(&Binding) -> bool,
) -> Option<Lookup> {
    let map = maps.get(map)?;
    match map.bindings.get(key) {
        Some(Binding::Nothing) => return Some(Lookup::Nothing),
        Some(Binding::Multi) => return Some(Lookup::Multi),
        Some(b) => {
            if handle(b) {
                return Some(Lookup::Handled);
            }
        }
        None => {}
    }
    for f in &map.fallthrough {
        if let Some(r) = lookup_key(maps, key, f, handle) {
            return Some(r);
        }
    }
    None
}

/// `keyNames[keyCode]` (src/input/keynames.js): the base key name of a DOM
/// `KeyboardEvent.keyCode`, or `None` for codes without a name.
pub fn key_names(code: u32) -> Option<String> {
    let s = match code {
        3 | 19 => "Pause",
        8 => "Backspace",
        9 => "Tab",
        13 => "Enter",
        16 => "Shift",
        17 => "Ctrl",
        18 => "Alt",
        20 => "CapsLock",
        27 => "Esc",
        32 => "Space",
        33 | 63276 => "PageUp",
        34 | 63277 => "PageDown",
        35 | 63275 => "End",
        36 | 63273 => "Home",
        37 | 63234 => "Left",
        38 | 63232 => "Up",
        39 | 63235 => "Right",
        40 | 63233 => "Down",
        44 => "PrintScrn",
        45 | 63302 => "Insert",
        46 | 63272 => "Delete",
        59 | 186 => ";",
        61 | 107 | 187 => "=",
        91 | 92 | 93 | 224 => "Mod",
        106 => "*",
        109 | 173 | 189 => "-",
        110 | 190 => ".",
        111 | 191 => "/",
        145 => "ScrollLock",
        188 => ",",
        192 => "`",
        219 => "[",
        220 => "\\",
        221 => "]",
        222 => "'",
        // Number keys.
        48..=57 => return Some((code - 48).to_string()),
        96..=105 => return Some((code - 96).to_string()),
        // Alphabetic keys.
        65..=90 => return char::from_u32(code).map(|c| c.to_string()),
        // Function keys.
        112..=123 => return Some(format!("F{}", code - 111)),
        63236..=63247 => return Some(format!("F{}", code - 63235)),
        _ => return None,
    };
    Some(s.to_string())
}

/// The DOM half of `keyName(event)`: a [`KeyEvent`] from `keyCode`, `code` and the
/// modifier flags `[shift, ctrl, alt, meta]` of a `KeyboardEvent`, or `None` where
/// `keyName` returns false (no name for the code, or `altGraphKey`). Ctrl-ScrollLock
/// has keyCode 3, like Ctrl-Pause, so code 3 takes its name from `event.code` when
/// there is one.
pub fn key_event_from_dom(
    key_code: u32,
    code: Option<&str>,
    mods: [bool; 4],
    alt_graph: bool,
) -> Option<KeyEvent> {
    let mut key = key_names(key_code)?;
    if alt_graph {
        return None;
    }
    if key_code == 3
        && let Some(c) = code.filter(|c| !c.is_empty())
    {
        key = c.to_string();
    }
    let [shift, ctrl, alt, meta] = mods;
    Some(KeyEvent {
        key,
        shift,
        ctrl,
        alt,
        meta,
        alt_graph,
    })
}

/// `isModifierKey(value)`.
pub fn is_modifier_key(name: &str) -> bool {
    matches!(name, "Ctrl" | "Alt" | "Shift" | "Mod")
}

/// A key event as CM5 sees it: the base key name from `keyNames` (`"Left"`, `"A"`,
/// `"Enter"`, `"Space"`, `"F5"`, `"["`, ...) and the modifier flags.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyEvent {
    pub key: String,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
    /// `altGraphKey`.
    pub alt_graph: bool,
}

impl KeyEvent {
    pub fn new(key: &str) -> KeyEvent {
        KeyEvent {
            key: key.to_string(),
            ..Default::default()
        }
    }
    /// Parse `"Shift-Ctrl-Left"` style descriptions (test convenience).
    pub fn parse(desc: &str) -> KeyEvent {
        let n = normalize_key_name(desc).expect("valid key");
        let mut e = KeyEvent::default();
        let mut rest = n.as_str();
        loop {
            if let Some(r) = rest.strip_prefix("Shift-") {
                e.shift = true;
                rest = r;
            } else if let Some(r) = rest.strip_prefix("Cmd-") {
                e.meta = true;
                rest = r;
            } else if let Some(r) = rest.strip_prefix("Ctrl-") {
                e.ctrl = true;
                rest = r;
            } else if let Some(r) = rest.strip_prefix("Alt-") {
                e.alt = true;
                rest = r;
            } else {
                break;
            }
        }
        e.key = rest.to_string();
        e
    }
}

/// `addModifierNames(name, event, noShift)`; `flip_ctrl_cmd` is the JS `flipCtrlCmd`
/// (mac).
pub fn add_modifier_names(name: &str, e: &KeyEvent, no_shift: bool, flip_ctrl_cmd: bool) -> String {
    let base = name;
    let mut name = name.to_string();
    if e.alt && base != "Alt" {
        name = format!("Alt-{name}");
    }
    if (if flip_ctrl_cmd { e.meta } else { e.ctrl }) && base != "Ctrl" {
        name = format!("Ctrl-{name}");
    }
    if (if flip_ctrl_cmd { e.ctrl } else { e.meta }) && base != "Mod" {
        name = format!("Cmd-{name}");
    }
    if !no_shift && e.shift && base != "Shift" {
        name = format!("Shift-{name}");
    }
    name
}

/// `keyName(event, noShift)`.
pub fn key_name(e: &KeyEvent, no_shift: bool, flip_ctrl_cmd: bool) -> Option<String> {
    if e.key.is_empty() || e.alt_graph {
        return None;
    }
    Some(add_modifier_names(&e.key, e, no_shift, flip_ctrl_cmd))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize() {
        assert_eq!(normalize_key_name("Ctrl-Shift-a").unwrap(), "Shift-Ctrl-a");
        assert_eq!(
            normalize_key_name("Cmd-Alt-Ctrl-Shift-X").unwrap(),
            "Shift-Cmd-Ctrl-Alt-X"
        );
        assert_eq!(normalize_key_name("Ctrl--").unwrap(), "Ctrl--");
        assert_eq!(normalize_key_name("-").unwrap(), "-");
        assert!(normalize_key_name("Hyper-X").is_err());
    }

    #[test]
    fn dom_key_names() {
        // Every entry of src/input/keynames.js, including the loops.
        let table: &[(u32, &str)] = &[
            (3, "Pause"),
            (8, "Backspace"),
            (9, "Tab"),
            (13, "Enter"),
            (16, "Shift"),
            (17, "Ctrl"),
            (18, "Alt"),
            (19, "Pause"),
            (20, "CapsLock"),
            (27, "Esc"),
            (32, "Space"),
            (33, "PageUp"),
            (34, "PageDown"),
            (35, "End"),
            (36, "Home"),
            (37, "Left"),
            (38, "Up"),
            (39, "Right"),
            (40, "Down"),
            (44, "PrintScrn"),
            (45, "Insert"),
            (46, "Delete"),
            (59, ";"),
            (61, "="),
            (91, "Mod"),
            (92, "Mod"),
            (93, "Mod"),
            (106, "*"),
            (107, "="),
            (109, "-"),
            (110, "."),
            (111, "/"),
            (145, "ScrollLock"),
            (173, "-"),
            (186, ";"),
            (187, "="),
            (188, ","),
            (189, "-"),
            (190, "."),
            (191, "/"),
            (192, "`"),
            (219, "["),
            (220, "\\"),
            (221, "]"),
            (222, "'"),
            (224, "Mod"),
            (63232, "Up"),
            (63233, "Down"),
            (63234, "Left"),
            (63235, "Right"),
            (63272, "Delete"),
            (63273, "Home"),
            (63275, "End"),
            (63276, "PageUp"),
            (63277, "PageDown"),
            (63302, "Insert"),
        ];
        let mut want: HashMap<u32, String> =
            table.iter().map(|(c, n)| (*c, n.to_string())).collect();
        for i in 0..10 {
            want.insert(i + 48, i.to_string());
            want.insert(i + 96, i.to_string());
        }
        for i in 65..=90 {
            want.insert(i, char::from_u32(i).unwrap().to_string());
        }
        for i in 1..=12 {
            want.insert(i + 111, format!("F{i}"));
            want.insert(i + 63235, format!("F{i}"));
        }
        for code in 0..70000 {
            assert_eq!(key_names(code), want.get(&code).cloned(), "keyCode {code}");
        }
        let e = key_event_from_dom(3, Some("ScrollLock"), [false, true, false, false], false);
        assert_eq!(e.unwrap().key, "ScrollLock");
        let e = key_event_from_dom(65, Some("KeyA"), [true, true, false, false], false).unwrap();
        assert_eq!(key_name(&e, false, false).as_deref(), Some("Shift-Ctrl-A"));
        assert_eq!(key_event_from_dom(65, None, [false; 4], true), None);
        assert_eq!(key_event_from_dom(229, None, [false; 4], false), None);
    }
}
