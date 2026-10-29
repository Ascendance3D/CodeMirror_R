//! Shared C4 oracle harness: replays `oracle/cm5-<key>-baseline.json` (written by
//! `oracle/cm5-modes-tokens.mjs`) through the Rust modes. Must stay in step with the
//! `run` / `digest` / `readText` functions there.
#![allow(dead_code)]

use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use cm_core::text::to_u16;
use cm_core::util::string_stream::StringStream;
use cm_modes::{
    ConfigValue, JsArg, LinesOracle, ModeOptions, ModeSpec, Registry, Spec, split_lines_runmode,
    start_state,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A JSON string, or an array of UTF-16 code units for ill-formed text.
pub fn units(v: &Value) -> Vec<u16> {
    match v {
        Value::String(s) => to_u16(s),
        Value::Array(a) => a.iter().map(|x| x.as_u64().unwrap() as u16).collect(),
        other => panic!("bad text {other}"),
    }
}

pub fn enc(u: &[u16]) -> Value {
    match String::from_utf16(u) {
        Ok(s) => Value::String(s),
        Err(_) => Value::Array(u.iter().map(|&c| json!(c)).collect()),
    }
}

/// A spec as the oracle writes it: a string, `"@id"` for [`special`], or an object of
/// plain JSON options.
pub fn spec_from_json(v: &Value, special: &dyn Fn(&str) -> Spec) -> Spec {
    match v {
        Value::String(s) if s.starts_with('@') => special(s),
        Value::String(s) => Spec::from(s.as_str()),
        Value::Null => Spec::Null,
        Value::Object(o) => {
            let mut spec = ModeSpec::named(o["name"].as_str().expect("spec name"));
            for (k, v) in o {
                if k == "name" {
                    continue;
                }
                if k == "helperType" {
                    spec.helper_type = v.as_str().map(str::to_string);
                    continue;
                }
                let cv = match v {
                    Value::Null => ConfigValue::Null,
                    Value::Bool(b) => ConfigValue::Bool(*b),
                    Value::Number(n) => ConfigValue::Num(n.as_f64().unwrap()),
                    Value::String(s) => ConfigValue::Str(s.clone()),
                    other => panic!("spec option {k} = {other} needs an @id"),
                };
                spec.options.insert(k.clone(), cv);
            }
            Spec::Obj(spec)
        }
        other => panic!("bad spec {other}"),
    }
}

pub fn indent_value(v: Option<f64>) -> String {
    match v {
        None => "Pass".into(),
        Some(x) if x.is_nan() => "NaN".into(),
        Some(x) if x.fract() == 0.0 && x.abs() < 1e15 => format!("{}", x as i64),
        Some(x) => format!("{x}"),
    }
}

pub struct RunResult {
    pub tokens: Vec<Value>,
    pub indents: Option<Vec<String>>,
    pub error: Option<String>,
}

/// The oracle's `run`: runMode plus the indent hook and the same no-progress guard.
pub fn run(registry: &Registry, text: &[u16], spec: &Spec) -> RunResult {
    let mut tokens = Vec::new();
    let mut indents = Vec::new();
    let options = ModeOptions::default();
    let mode = registry.get_mode(&options, spec);
    let has_indent = mode.has_indent();
    let lines = split_lines_runmode(text);
    let r = catch_unwind(AssertUnwindSafe(|| {
        let mut state = start_state(&*mode, JsArg::Undefined, JsArg::Undefined);
        let mut guard = 0;
        for i in 0..lines.len() {
            if i > 0 {
                tokens.push(json!(["\n"]));
            }
            if has_indent {
                let line = &lines[i];
                let ws = line
                    .iter()
                    .take_while(|&&c| cm_core::text::is_js_space(c))
                    .count();
                indents.push(indent_value(mode.indent(&state, &line[ws..], line)));
            }
            let oracle = LinesOracle { lines: &lines, i };
            let mut stream = StringStream::new(&lines[i], None, Some(&oracle));
            if stream.string.is_empty() && mode.has_blank_line() {
                mode.blank_line(&mut state);
            }
            while !stream.eol() {
                let style = mode.token(&mut stream, &mut state);
                tokens.push(json!([enc(stream.current()), style, i, stream.start]));
                if stream.pos == stream.start {
                    guard += 1;
                    if guard > 1000 {
                        panic!("runMode spins: no progress");
                    }
                }
                stream.start = stream.pos;
            }
        }
    }));
    let error = r.err().map(|e| {
        e.downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    });
    RunResult {
        tokens,
        indents: has_indent.then_some(indents),
        error,
    }
}

fn utf16le(h: &mut Sha256, s: &[u16]) {
    let mut buf = Vec::with_capacity(s.len() * 2);
    for &c in s {
        buf.extend_from_slice(&c.to_le_bytes());
    }
    h.update(&buf);
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// The oracle's `digest`.
pub fn digest(r: &RunResult) -> (String, Option<String>) {
    let mut th = Sha256::new();
    for t in &r.tokens {
        let a = t.as_array().unwrap();
        if a.len() == 1 {
            utf16le(&mut th, &[0x0a]);
            continue;
        }
        let text = units(&a[0]);
        let style = a[1].as_str().unwrap_or("\u{0}");
        let mut rec = to_u16(&format!("{},{},{},{}|", a[2], a[3], text.len(), style));
        rec.extend_from_slice(&text);
        utf16le(&mut th, &rec);
    }
    let ih = r.indents.as_ref().map(|ind| {
        let mut ih = Sha256::new();
        for (i, v) in ind.iter().enumerate() {
            utf16le(&mut ih, &to_u16(&format!("{i}:{v}\n")));
        }
        hex(&ih.finalize())
    });
    (hex(&th.finalize()), ih)
}

/// The oracle's `readText`: gzip (all members) in memory, UTF-8 with U+FFFD, BOM kept.
pub fn read_text(path: &Path) -> (bool, Vec<u16>) {
    let mut bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let gz = bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b;
    if gz {
        let mut out = Vec::new();
        flate2::read::MultiGzDecoder::new(&bytes[..])
            .read_to_end(&mut out)
            .expect("gunzip");
        bytes = out;
    }
    (gz, to_u16(&String::from_utf8_lossy(&bytes)))
}

pub struct Report {
    pub cases: usize,
    pub files: usize,
    pub callbacks: usize,
    pub indent_lines: usize,
    pub token_mismatches: Vec<String>,
    pub indent_mismatches: Vec<String>,
    /// Lines where JS `indent()` returns the string `"CodeMirror.Pass" + n` and Rust
    /// NaN (see `cm_modes::js::JsNum::plus`): reported, not failed.
    pub pass_string_lines: Vec<String>,
    pub js_errors: usize,
}

/// Replays the whole baseline. Files are checked on `threads` worker threads.
pub fn check_baseline(key: &str, special: &(dyn Fn(&str) -> Spec + Sync)) -> Report {
    let path = root().join(format!("oracle/cm5-{key}-baseline.json"));
    let base: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("baseline")).unwrap();
    let registry = Registry::with_all_modes();
    let mut rep = Report {
        cases: 0,
        files: 0,
        callbacks: 0,
        indent_lines: 0,
        token_mismatches: vec![],
        indent_mismatches: vec![],
        pass_string_lines: vec![],
        js_errors: 0,
    };
    for (i, c) in base["cases"].as_array().unwrap().iter().enumerate() {
        rep.cases += 1;
        let text = units(&c["text"]);
        let spec = spec_from_json(&c["spec"], special);
        let got = run(&registry, &text, &spec);
        let want = c["tokens"].as_array().unwrap();
        rep.callbacks += want.len();
        let js_err = !c["error"].is_null();
        rep.js_errors += js_err as usize;
        if &got.tokens != want || js_err != got.error.is_some() {
            let k = got
                .tokens
                .iter()
                .zip(want)
                .position(|(a, b)| a != b)
                .unwrap_or(got.tokens.len().min(want.len()));
            rep.token_mismatches.push(format!(
                "{key} case {i} ({}, spec {}): first difference at callback {k}: js {:?} rust {:?} (js error {:?}, rust error {:?})",
                c["kind"], c["spec"], want.get(k), got.tokens.get(k), c["error"], got.error
            ));
        }
        if let Some(wi) = c["indents"].as_array() {
            rep.indent_lines += wi.len();
            let gi = got.indents.clone().unwrap_or_default();
            for (k, (w, g)) in wi.iter().zip(&gi).enumerate() {
                if w.as_str()
                    .is_some_and(|w| w.starts_with("string:CodeMirror.Pass"))
                    && g == "NaN"
                {
                    rep.pass_string_lines
                        .push(format!("{key} case {i} line {k}: js {w} rust NaN"));
                }
            }
            let same = |w: Option<&str>, g: Option<&str>| {
                w == g
                    || w.is_some_and(|w| w.starts_with("string:CodeMirror.Pass"))
                        && g == Some("NaN")
            };
            if let Some(k) = (0..wi.len().max(gi.len())).find(|&k| {
                !same(
                    wi.get(k).and_then(Value::as_str),
                    gi.get(k).map(String::as_str),
                )
            }) {
                rep.indent_mismatches.push(format!(
                    "{key} case {i} ({}): indent of line {k}: js {:?} rust {:?}",
                    c["kind"],
                    wi.get(k),
                    gi.get(k)
                ));
            }
        } else if got.indents.is_some() {
            rep.indent_mismatches
                .push(format!("{key} case {i}: rust mode has indent, js does not"));
        }
    }
    // Files below oracle/corpus-items are a private input (Ascendance3D/X_ITE_R#168): found and
    // verified by cm-evidence, or (ci mode, allowed by name) skipped once and logged.
    const PRIVATE: &str = "oracle/corpus-items/";
    let all = base["files"].as_array().unwrap();
    let corpus = if all
        .iter()
        .any(|f| f["path"].as_str().unwrap().starts_with(PRIVATE))
    {
        cm_evidence::private_input("corpus-items")
    } else {
        None
    };
    let files: Vec<Value> = all
        .iter()
        .filter(|f| corpus.is_some() || !f["path"].as_str().unwrap().starts_with(PRIVATE))
        .cloned()
        .collect();
    let corpus = &corpus;
    let file_path = move |p: &str| match (p.strip_prefix(PRIVATE), corpus) {
        (Some(rel), Some(dir)) => dir.join(rel),
        _ => root().join(p),
    };
    let threads = 12;
    let chunks: Vec<Vec<(usize, &Value)>> = (0..threads)
        .map(|t| {
            files
                .iter()
                .enumerate()
                .filter(|(i, _)| i % threads == t)
                .collect()
        })
        .collect();
    let results: Vec<(Vec<String>, Vec<String>)> = std::thread::scope(|s| {
        let hs: Vec<_> = chunks
            .iter()
            .map(|chunk| {
                s.spawn(move || {
                    let registry = Registry::with_all_modes();
                    let (mut tm, mut im) = (vec![], vec![]);
                    for (_, f) in chunk {
                        let p = f["path"].as_str().unwrap();
                        let (gz, text) = read_text(&file_path(p));
                        assert_eq!(gz, f["gz"].as_bool().unwrap(), "{p}");
                        assert_eq!(text.len() as u64, f["units"].as_u64().unwrap(), "{p}: decoded length");
                        let spec = spec_from_json(&f["spec"], special);
                        let got = run(&registry, &text, &spec);
                        let (th, ih) = digest(&got);
                        if th != f["tokenHash"].as_str().unwrap() || got.tokens.len() as u64 != f["callbacks"].as_u64().unwrap() || got.error.is_some() != !f["error"].is_null() {
                            tm.push(format!("{key} file {p}: token stream differs (js {} callbacks, rust {}; rust error {:?})", f["callbacks"], got.tokens.len(), got.error));
                        }
                        if ih.as_deref() != f["indentHash"].as_str() {
                            im.push(format!("{key} file {p}: indents differ"));
                        }
                    }
                    (tm, im)
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for f in &files {
        rep.files += 1;
        rep.callbacks += f["callbacks"].as_u64().unwrap() as usize;
        rep.js_errors += !f["error"].is_null() as usize;
        if !f["indentHash"].is_null() {
            rep.indent_lines += f["lines"].as_u64().unwrap() as usize;
        }
    }
    for (tm, im) in results {
        rep.token_mismatches.extend(tm);
        rep.indent_mismatches.extend(im);
    }
    rep
}

pub fn no_special(id: &str) -> Spec {
    panic!("unknown special spec {id}")
}

pub fn print_and_assert(key: &str, rep: &Report) {
    for m in rep
        .token_mismatches
        .iter()
        .chain(&rep.indent_mismatches)
        .take(40)
    {
        println!("{m}");
    }
    for m in &rep.pass_string_lines {
        println!("known divergence: {m}");
    }
    println!(
        "{key} oracle: {} cases + {} files, {} callbacks, {} indent lines; {} token mismatches, {} indent mismatches, {} JS errors, {} known Pass+n string indents",
        rep.cases,
        rep.files,
        rep.callbacks,
        rep.indent_lines,
        rep.token_mismatches.len(),
        rep.indent_mismatches.len(),
        rep.js_errors,
        rep.pass_string_lines.len()
    );
    assert!(rep.token_mismatches.is_empty() && rep.indent_mismatches.is_empty());
}

// --------------------------------------------------------------------------------
// test/mode_test.js

/// `styleName = /[\w&-_]+/g`: note `&-_` is the range U+0026..U+005F.
fn style_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || ('&'..='_').contains(&c)
}

/// `findSingle(str, pos, ch)`.
fn find_single(s: &[char], mut pos: usize, ch: char) -> Option<usize> {
    loop {
        let found = (pos..s.len()).find(|&i| s[i] == ch)?;
        if s.get(found + 1) != Some(&ch) {
            return Some(found);
        }
        pos = found + 2;
    }
}

/// `parseTokens(strs)` -> (expected tokens, plain text).
pub fn parse_tokens(strs: &[&str]) -> (Vec<(Option<String>, String)>, String) {
    let mut tokens = Vec::new();
    let mut plain = String::new();
    for (i, s) in strs.iter().enumerate() {
        if i > 0 {
            plain.push('\n');
        }
        let s: Vec<char> = s.chars().collect();
        let mut pos = 0;
        while pos < s.len() {
            let (style, text);
            if s[pos] == '[' && s.get(pos + 1) != Some(&'[') {
                let n = s[pos + 1..]
                    .iter()
                    .take_while(|&&c| style_name_char(c))
                    .count();
                let st: String = s[pos + 1..pos + 1 + n]
                    .iter()
                    .collect::<String>()
                    .replace('&', " ");
                let text_start = pos + st.chars().count() + 2;
                let end = find_single(&s, text_start, ']')
                    .unwrap_or_else(|| panic!("Unterminated token at {pos}"));
                text = s[text_start..end].iter().collect::<String>();
                style = Some(st);
                pos = end + 1;
            } else {
                let end = find_single(&s, pos, '[').unwrap_or(s.len());
                text = s[pos..end].iter().collect::<String>();
                style = None;
                pos = end;
            }
            let text = text.replace("[[", "[").replace("]]", "]");
            plain.push_str(&text);
            tokens.push((style, text));
        }
    }
    (tokens, plain)
}

fn sort_style(s: &str) -> String {
    let mut parts: Vec<&str> = s.split(' ').collect();
    parts.sort();
    parts.join(" ")
}

/// `test.mode(name, mode, tokens)`: highlight + compare + indentation check.
pub fn mode_test(
    registry: &Registry,
    spec: &Spec,
    indent_unit: i64,
    strs: &[&str],
) -> Result<(), String> {
    let (expected, text) = parse_tokens(strs);
    // `if (sty && sty.indexOf(" ")) sty = sorted` — always, unless it starts with " ".
    let expected: Vec<(Option<String>, String)> = expected
        .into_iter()
        .map(|(s, t)| {
            (
                s.map(|s| {
                    if s.starts_with(' ') {
                        s
                    } else {
                        sort_style(&s)
                    }
                }),
                t,
            )
        })
        .collect();
    let options = ModeOptions {
        indent_unit,
        ..ModeOptions::default()
    };
    let mode = registry.get_mode(&options, spec);
    let mut state = start_state(&*mode, JsArg::Undefined, JsArg::Undefined);
    let lines: Vec<Vec<u16>> = text.replace("\r\n", "\n").split('\n').map(to_u16).collect();
    let mut st: Vec<(Option<String>, Vec<u16>)> = Vec::new();
    let mut failures = Vec::new();
    for i in 0..lines.len() {
        let line = &lines[i];
        let mut new_line = true;
        if mode.has_indent() {
            let ws = line
                .iter()
                .take_while(|&&c| cm_core::text::is_js_space(c))
                .count();
            let indent = mode.indent(&state, &line[ws..], line);
            if let Some(ind) = indent {
                if ind != ws as f64 {
                    failures.push(format!(
                        "Indentation of line {} is {} (expected {ws})",
                        i + 1,
                        indent_value(Some(ind))
                    ));
                }
            }
        }
        let oracle = LinesOracle { lines: &lines, i };
        let mut stream = StringStream::new(line, Some(4), Some(&oracle));
        if line.is_empty() && mode.has_blank_line() {
            mode.blank_line(&mut state);
        }
        while !stream.eol() {
            let mut compare = None;
            let mut j = 0;
            while j < 10 && stream.start >= stream.pos {
                compare = mode.token(&mut stream, &mut state);
                j += 1;
            }
            if j == 10 {
                return Err(format!("Failed to advance the stream. line {i}"));
            }
            let substr = stream.current().to_vec();
            let compare = compare.map(|c| if c.contains(' ') { sort_style(&c) } else { c });
            stream.start = stream.pos;
            if !st.is_empty() && st.last().unwrap().0 == compare && !new_line {
                st.last_mut().unwrap().1.extend_from_slice(&substr);
            } else if !substr.is_empty() {
                st.push((compare, substr));
            }
            new_line = false;
        }
    }
    let observed: Vec<(Option<String>, String)> = st
        .into_iter()
        .map(|(s, t)| (s, String::from_utf16_lossy(&t)))
        .collect();
    if observed != expected {
        let k = observed
            .iter()
            .zip(&expected)
            .position(|(a, b)| a != b)
            .unwrap_or(observed.len().min(expected.len()));
        failures.insert(
            0,
            format!(
                "token {k}: expected {:?}, observed {:?}",
                expected.get(k),
                observed.get(k)
            ),
        );
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}
