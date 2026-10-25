//! UTF-16 helpers. CodeMirror measures everything in UTF-16 code units, so the
//! document stores line text as `Vec<u16>`.

/// One line of text as UTF-16 code units.
pub type Text = Vec<u16>;

pub fn to_u16(s: &str) -> Text {
    s.encode_utf16().collect()
}

/// Lossy: lone surrogates become U+FFFD.
pub fn from_u16(s: &[u16]) -> String {
    String::from_utf16_lossy(s)
}

/// JavaScript `String.prototype.slice(start, end)` on UTF-16 units, including
/// negative-offset and clamping semantics.
pub fn js_slice(s: &[u16], start: i64, end: Option<i64>) -> &[u16] {
    let len = s.len() as i64;
    let norm = |v: i64| if v < 0 { (len + v).max(0) } else { v.min(len) };
    let a = norm(start);
    let b = end.map(norm).unwrap_or(len);
    if b <= a {
        &[]
    } else {
        &s[a as usize..b as usize]
    }
}

/// JavaScript `indexOf(needle, from)` on UTF-16 units.
pub fn index_of(hay: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    let from = from.min(hay.len());
    if needle.is_empty() {
        return Some(from);
    }
    if needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// JavaScript `\s` (non-unicode-mode regexps): WhiteSpace + LineTerminator.
pub fn is_js_space(c: u16) -> bool {
    matches!(c,
        0x09..=0x0d | 0x20 | 0xa0 | 0x1680 | 0x2000..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f
        | 0x3000 | 0xfeff)
}

/// JavaScript `\w` (non-unicode-mode): `[A-Za-z0-9_]`.
pub fn is_js_word(c: u16) -> bool {
    matches!(c, 0x30..=0x39 | 0x41..=0x5a | 0x61..=0x7a | 0x5f)
}

/// JavaScript `.`: anything but a line terminator.
pub fn is_js_dot(c: u16) -> bool {
    !matches!(c, 0x0a | 0x0d | 0x2028 | 0x2029)
}

/// `str.split(/\r\n?|\n/)` — `splitLinesAuto` in `src/util/feature_detection.js` on
/// every browser where `"\n\nb".split(/\n/).length == 3`.
pub fn split_lines_auto(s: &[u16]) -> Vec<Text> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == 0x0d {
            out.push(std::mem::take(&mut cur));
            if i + 1 < s.len() && s[i + 1] == 0x0a {
                i += 1;
            }
        } else if c == 0x0a {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
        i += 1;
    }
    out.push(cur);
    out
}

/// `str.split(sep)` with a string separator.
pub fn split_by(s: &[u16], sep: &[u16]) -> Vec<Text> {
    if sep.is_empty() {
        // "abc".split("") -> ["a", "b", "c"]; "".split("") -> []
        return s.iter().map(|&c| vec![c]).collect();
    }
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(f) = index_of(s, sep, pos) {
        out.push(s[pos..f].to_vec());
        pos = f + sep.len();
    }
    out.push(s[pos..].to_vec());
    out
}

pub fn join(lines: &[Text], sep: &[u16]) -> Text {
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(sep);
        }
        out.extend_from_slice(l);
    }
    out
}
