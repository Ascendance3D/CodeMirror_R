//! `src/input/keymap.js` and `src/input/keynames.js`: key maps, key name
//! normalization and lookup with fallthrough.

use std::collections::HashMap;
use std::rc::Rc;

use crate::editor::Editor;

/// A handler function bound in a key map (`keymap["X"] = function(cm) {...}`).
/// Returning `false` is `CodeMirror.Pass`.
pub type KeyFn = Rc<dyn Fn(&mut Editor) -> bool>;

/// A key map value.
#[derive(Clone)]
pub enum Binding {
    /// A command name.
    Command(String),
    /// A function.
    Fn(KeyFn),
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
}
