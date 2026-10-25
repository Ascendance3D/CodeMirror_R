//! C2 differential oracle replay.
//!
//! `oracle/cm5-doc-ops.mjs` ran seeded operation sequences on the real CodeMirror 5
//! `Doc` in headless chromium and stored the operations plus `JSON.stringify` of the
//! results. This test replays every operation on the Rust port and serializes the
//! results with a `JSON.stringify`-compatible writer; the strings must be identical.
//!
//! Baseline: `oracle/cm5-doc-ops-baseline.json` (override with `CM_FUZZ_BASELINE`).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use cm_core::clock;
use cm_core::{
    Doc, DocOptions, EditorSemantics, HistoryEventData, LinkedDocOptions, MarkOptions, Marker,
    MarkerFind, Pos, SelOptions, Sticky,
};
use serde_json::Value;

// ---------------------------------------------------------------------------------
// A JSON.stringify-compatible writer over UTF-16 strings.

enum J {
    Null,
    Bool(bool),
    Num(i64),
    Str(Vec<u16>),
    Arr(Vec<J>),
    Obj(Vec<(&'static str, J)>),
}

fn s16(s: &str) -> J {
    J::Str(s.encode_utf16().collect())
}

fn write_str(out: &mut String, s: &[u16]) {
    out.push('"');
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        match c {
            0x22 => out.push_str("\\\""),
            0x5c => out.push_str("\\\\"),
            0x08 => out.push_str("\\b"),
            0x0c => out.push_str("\\f"),
            0x0a => out.push_str("\\n"),
            0x0d => out.push_str("\\r"),
            0x09 => out.push_str("\\t"),
            c if c < 0x20 => out.push_str(&format!("\\u{c:04x}")),
            0xd800..=0xdbff if i + 1 < s.len() && (0xdc00..=0xdfff).contains(&s[i + 1]) => {
                let cp = 0x10000 + (((c as u32) - 0xd800) << 10) + (s[i + 1] as u32 - 0xdc00);
                out.push(char::from_u32(cp).unwrap());
                i += 1;
            }
            0xd800..=0xdfff => out.push_str(&format!("\\u{c:04x}")),
            c => out.push(char::from_u32(c as u32).unwrap()),
        }
        i += 1;
    }
    out.push('"');
}

fn write(out: &mut String, j: &J) {
    match j {
        J::Null => out.push_str("null"),
        J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        J::Num(n) => out.push_str(&n.to_string()),
        J::Str(s) => write_str(out, s),
        J::Arr(a) => {
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(out, v);
            }
            out.push(']');
        }
        J::Obj(o) => {
            out.push('{');
            for (i, (k, v)) in o.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(out, &k.encode_utf16().collect::<Vec<_>>());
                out.push(':');
                write(out, v);
            }
            out.push('}');
        }
    }
}

// ---------------------------------------------------------------------------------
// Replay state.

struct St {
    docs: Vec<Doc>,
    marks: Vec<Marker>,
    labels: Vec<Marker>,
}

impl St {
    fn label(&mut self, m: &Marker) -> J {
        if let Some(i) = self.labels.iter().position(|x| x == m) {
            return J::Num(i as i64);
        }
        self.labels.push(m.clone());
        J::Num(self.labels.len() as i64 - 1)
    }
}

fn p(p: Pos) -> J {
    J::Arr(vec![
        J::Num(p.line),
        J::Num(p.ch),
        match p.sticky {
            None => J::Null,
            Some(s) => s16(s.as_str()),
        },
    ])
}

fn sel(d: &Doc) -> J {
    let s = d.selection();
    J::Arr(vec![
        J::Arr(
            s.ranges
                .iter()
                .map(|r| J::Arr(vec![p(r.anchor), p(r.head)]))
                .collect(),
        ),
        J::Num(s.primary as i64),
    ])
}

fn find(m: &Marker) -> J {
    match m.find() {
        None => J::Null,
        Some(MarkerFind::Point(x)) => p(x),
        Some(MarkerFind::Range { from, to }) => J::Arr(vec![p(from), to.map(p).unwrap_or(J::Null)]),
    }
}

fn hist(d: &Doc) -> J {
    let h = d.get_history();
    let ev = |e: &HistoryEventData| match e {
        HistoryEventData::Selection(s) => J::Arr(vec![
            s16("s"),
            J::Arr(
                s.ranges
                    .iter()
                    .map(|r| J::Arr(vec![p(r.anchor), p(r.head)]))
                    .collect(),
            ),
            J::Num(s.primary as i64),
        ]),
        HistoryEventData::Changes(cs) => J::Arr(vec![
            s16("c"),
            J::Arr(
                cs.iter()
                    .map(|c| {
                        let mut t: Vec<u16> = Vec::new();
                        for (i, l) in c.text.iter().enumerate() {
                            if i > 0 {
                                t.push(0x0a);
                            }
                            t.extend_from_slice(l);
                        }
                        J::Arr(vec![p(c.from), p(c.to), J::Str(t)])
                    })
                    .collect(),
            ),
        ]),
    };
    J::Arr(vec![
        J::Arr(h.done.iter().map(ev).collect()),
        J::Arr(h.undone.iter().map(ev).collect()),
    ])
}

fn snap(d: &Doc) -> J {
    let hs = d.history_size();
    J::Arr(vec![
        J::Str(d.get_value_utf16(None)),
        sel(d),
        J::Num(hs.undo as i64),
        J::Num(hs.redo as i64),
    ])
}

fn mk(v: &Value) -> Option<Pos> {
    let a = v.as_array()?;
    let line = a[0].as_i64().unwrap();
    if a[1].is_null() {
        return Some(Pos::line_end(line));
    }
    let sticky = match a[2].as_str() {
        Some("before") => Some(Sticky::Before),
        Some("after") => Some(Sticky::After),
        _ => None,
    };
    Some(Pos::with_sticky(line, a[1].as_i64().unwrap(), sticky))
}

fn opt_str(v: &Value) -> Option<&str> {
    v.as_str()
}

fn sel_opts(v: &Value) -> Option<SelOptions> {
    if v.is_null() {
        return None;
    }
    Some(SelOptions {
        origin: v["origin"].as_str().map(str::to_string),
        bias: v["bias"].as_i64(),
        scroll: None,
        clear_redo: v["clearRedo"].as_bool(),
    })
}

fn mark_opts(v: &Value) -> MarkOptions {
    let b = |k: &str| v[k].as_bool().unwrap_or(false);
    MarkOptions {
        inclusive_left: b("inclusiveLeft"),
        inclusive_right: b("inclusiveRight"),
        atomic: b("atomic"),
        read_only: b("readOnly"),
        clear_when_empty: v["clearWhenEmpty"].as_bool(),
        add_to_history: b("addToHistory"),
        collapsed: b("collapsed"),
        clear_on_enter: b("clearOnEnter"),
        select_left: v["selectLeft"].as_bool(),
        select_right: v["selectRight"].as_bool(),
        shared: b("shared"),
        ..Default::default()
    }
}

fn err_j(msg: String) -> J {
    J::Obj(vec![("err", s16(&msg))])
}

fn apply(st: &mut St, o: &Value, now: &std::rc::Rc<std::cell::Cell<f64>>) -> J {
    let op = o["op"].as_str().unwrap();
    let di = o["d"].as_u64().unwrap_or(0) as usize;
    let d = st
        .docs
        .get(di)
        .cloned()
        .unwrap_or_else(|| st.docs[0].clone());
    match op {
        "replaceRange" => {
            d.replace_range(
                o["text"].as_str().unwrap(),
                mk(&o["from"]).unwrap(),
                mk(&o["to"]),
                opt_str(&o["origin"]),
            );
            snap(&d)
        }
        "replaceSelection" => {
            d.replace_selection(
                o["text"].as_str().unwrap(),
                opt_str(&o["collapse"]),
                opt_str(&o["origin"]),
            );
            snap(&d)
        }
        "setSelection" => {
            d.set_selection(
                mk(&o["anchor"]).unwrap(),
                mk(&o["head"]),
                sel_opts(&o["opts"]),
            );
            snap(&d)
        }
        "setCursor" => {
            d.set_cursor(mk(&o["pos"]).unwrap(), sel_opts(&o["opts"]));
            snap(&d)
        }
        "setSelections" => {
            let ranges: Vec<(Pos, Option<Pos>)> = o["ranges"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| (mk(&r[0]).unwrap(), mk(&r[1])))
                .collect();
            d.set_selections(
                &ranges,
                o["primary"].as_u64().map(|x| x as usize),
                sel_opts(&o["opts"]),
            );
            snap(&d)
        }
        "addSelection" => {
            d.add_selection(
                mk(&o["anchor"]).unwrap(),
                mk(&o["head"]),
                sel_opts(&o["opts"]),
            );
            snap(&d)
        }
        "extendSelection" => {
            d.extend_selection(
                mk(&o["head"]).unwrap(),
                mk(&o["other"]),
                sel_opts(&o["opts"]),
            );
            snap(&d)
        }
        "extendSelections" => {
            let heads: Vec<Pos> = o["heads"]
                .as_array()
                .unwrap()
                .iter()
                .map(|h| mk(h).unwrap())
                .collect();
            d.extend_selections(&heads, None);
            snap(&d)
        }
        "undo" => {
            d.undo();
            snap(&d)
        }
        "redo" => {
            d.redo();
            snap(&d)
        }
        "undoSelection" => {
            d.undo_selection();
            snap(&d)
        }
        "redoSelection" => {
            d.redo_selection();
            snap(&d)
        }
        "setValue" => {
            d.set_value(o["text"].as_str().unwrap());
            snap(&d)
        }
        "selectAll" => {
            d.select_all();
            snap(&d)
        }
        "markText" => match d.mark_text(
            mk(&o["from"]).unwrap(),
            mk(&o["to"]).unwrap(),
            mark_opts(&o["opts"]),
        ) {
            Ok(m) => {
                st.marks.push(m.clone());
                let l = st.label(&m);
                J::Arr(vec![l, find(&m), snap(&d)])
            }
            Err(e) => err_j(e.to_string()),
        },
        "setBookmark" => match d.set_bookmark(
            mk(&o["pos"]).unwrap(),
            o["insertLeft"].as_bool().unwrap(),
            o["shared"].as_bool().unwrap(),
        ) {
            Ok(m) => {
                st.marks.push(m.clone());
                let l = st.label(&m);
                J::Arr(vec![l, find(&m), snap(&d)])
            }
            Err(e) => err_j(e.to_string()),
        },
        "clearMarker" => {
            let m = st.marks[o["m"].as_u64().unwrap() as usize].clone();
            m.clear();
            let l = st.label(&m);
            J::Arr(vec![l, snap(&st.docs[0])])
        }
        "markerFind" => {
            let m = st.marks[o["m"].as_u64().unwrap() as usize].clone();
            let l = st.label(&m);
            J::Arr(vec![l, find(&m)])
        }
        "tick" => {
            now.set(now.get() + o["ms"].as_f64().unwrap());
            J::Null
        }
        "clearHistory" => {
            d.clear_history();
            snap(&d)
        }
        "markClean" => {
            d.mark_clean();
            J::Bool(d.is_clean(None))
        }
        "changeGeneration" => J::Num(d.change_generation(o["force"].as_bool().unwrap()) as i64),
        "isClean" => J::Bool(d.is_clean(None)),
        "linkedDoc" => {
            let opts = LinkedDocOptions {
                shared_hist: o["sharedHist"].as_bool().unwrap(),
                from: o["from"].as_i64(),
                to: o["to"].as_i64(),
                mode: None,
            };
            match d.linked_doc(opts) {
                Ok(n) => {
                    st.docs.push(n.clone());
                    snap(&n)
                }
                Err(cm_core::CmError::TypeError(_)) => err_j("TypeError".into()),
                Err(e) => err_j(e.to_string()),
            }
        }
        "unlinkDoc" => {
            let other = st.docs[o["other"].as_u64().unwrap() as usize].clone();
            d.unlink_doc(&other);
            J::Arr(vec![snap(&d), snap(&other)])
        }
        "copyDoc" => {
            let n = d.copy(o["copyHistory"].as_bool().unwrap());
            st.docs.push(n.clone());
            J::Arr(vec![snap(&n), hist(&n)])
        }
        "historyRoundtrip" => {
            let h = d.get_history();
            d.set_history(&h);
            hist(&d)
        }
        "setExtending" => {
            d.set_extending(o["val"].as_bool().unwrap());
            J::Bool(d.get_extending())
        }
        "getRange" => J::Str(d.get_range_utf16(
            mk(&o["from"]).unwrap(),
            mk(&o["to"]).unwrap(),
            opt_str(&o["sep"]),
        )),
        "posFromIndex" => p(d.pos_from_index(o["index"].as_i64().unwrap())),
        "indexFromPos" => J::Num(d.index_from_pos(mk(&o["pos"]).unwrap())),
        "findMarksAt" => {
            let ms = d.find_marks_at(mk(&o["pos"]).unwrap());
            J::Arr(ms.iter().map(|m| st.label(m)).collect())
        }
        "findMarks" => {
            let ms = d.find_marks(mk(&o["from"]).unwrap(), mk(&o["to"]).unwrap(), None);
            J::Arr(ms.iter().map(|m| st.label(m)).collect())
        }
        "getCursor" => p(d.get_cursor(opt_str(&o["which"]))),
        "getSelections" => J::Arr(vec![
            J::Arr(
                d.get_selections_utf16(None)
                    .into_iter()
                    .map(J::Str)
                    .collect(),
            ),
            J::Str(d.get_selection_utf16(None)),
            J::Bool(d.something_selected()),
        ]),
        "getLine" => match d.get_line_utf16(o["line"].as_i64().unwrap()) {
            Some(l) => J::Str(l),
            None => J::Null,
        },
        "clipPos" => p(d.clip_pos(mk(&o["pos"]).unwrap())),
        "getAllMarks" => {
            let ms = d.get_all_marks();
            J::Arr(ms.into_iter().map(|m| st.label(&Marker::Text(m))).collect())
        }
        "getHistory" => hist(&d),
        "batch" => {
            let doc0 = st.docs[0].clone();
            // An exception inside `cm.operation` aborts the whole batch.
            doc0.operation(|| {
                let mut rs = Vec::new();
                for s in o["subs"].as_array().unwrap() {
                    let r = apply(st, s, now);
                    if let J::Obj(_) = r {
                        return r;
                    }
                    rs.push(r);
                }
                J::Arr(rs)
            })
        }
        other => panic!("unknown op {other}"),
    }
}

fn panic_msg(e: Box<dyn std::any::Any + Send>) -> String {
    let s = if let Some(s) = e.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = e.downcast_ref::<&str>() {
        s.to_string()
    } else {
        "panic".into()
    };
    if std::env::var("CM_FUZZ_RAW_ERRORS").is_ok() {
        return s;
    }
    if s.contains("TypeError") {
        "TypeError".into()
    } else {
        s
    }
}

/// Replays one sequence; returns the serialized results and the per-op strings
/// (for locating the first difference).
fn replay(seq: &Value) -> (String, Vec<String>) {
    let now = clock::manual(1e12);
    let init = &seq["init"];
    let doc = Doc::with_options(DocOptions {
        text: init["text"].as_str().unwrap().to_string(),
        first_line: init["firstLine"].as_i64().unwrap(),
        line_sep: init["lineSep"].as_str().map(str::to_string),
        ..Default::default()
    });
    if init["editor"].as_bool() == Some(true) {
        doc.attach_editor_semantics(EditorSemantics::default());
    }
    if let Some(depth) = init["undoDepth"].as_f64() {
        doc.set_undo_depth(depth);
    }
    let mut st = St {
        docs: vec![doc],
        marks: Vec::new(),
        labels: Vec::new(),
    };
    let mut outs: Vec<J> = Vec::new();
    let debug = std::env::var("CM_FUZZ_DEBUG_SEED")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        == seq["seed"].as_i64();
    for (k, o) in seq["ops"].as_array().unwrap().iter().enumerate() {
        let r = catch_unwind(AssertUnwindSafe(|| apply(&mut st, o, &now)));
        outs.push(match r {
            Ok(j) => j,
            Err(e) => {
                let m = panic_msg(e);
                if debug {
                    eprintln!("ERR {k} {m}");
                }
                err_j(if m.contains("TypeError") {
                    "TypeError".into()
                } else {
                    m
                })
            }
        });
        if debug {
            let docs = st.docs.clone();
            let mut dd = Vec::new();
            for d in &docs {
                let mut lines = Vec::new();
                for l in d.debug_spans() {
                    let mut sp = Vec::new();
                    for (m, f, t) in l {
                        let lab = st.label(&Marker::Text(m));
                        sp.push(J::Arr(vec![
                            lab,
                            f.map(J::Num).unwrap_or(J::Null),
                            t.map(J::Num).unwrap_or(J::Null),
                        ]));
                    }
                    lines.push(J::Arr(sp));
                }
                dd.push(J::Arr(lines));
            }
            let mut ml = Vec::new();
            for (i, m) in st.labels.iter().enumerate() {
                if let Marker::Text(t) = m {
                    ml.push(J::Arr(vec![
                        J::Num(i as i64),
                        J::Arr(
                            t.debug_line_numbers()
                                .into_iter()
                                .map(|x| x.map(J::Num).unwrap_or(J::Null))
                                .collect(),
                        ),
                    ]));
                }
            }
            let sz: Vec<J> = docs
                .iter()
                .map(|d| {
                    J::Arr(vec![
                        J::Num(d.first_line()),
                        J::Num(d.line_count() as i64),
                        J::Num(d.debug_actual_line_count() as i64),
                    ])
                })
                .collect();
            let mut s = String::new();
            let hi: Vec<J> = docs
                .iter()
                .map(|d| {
                    let h = d.debug_history_state();
                    J::Arr(vec![
                        h.0.map(|x| s16(&x)).unwrap_or(J::Null),
                        h.1.map(|x| s16(&x)).unwrap_or(J::Null),
                        J::Num(h.2 as i64),
                        J::Num(h.3 as i64),
                        J::Num(h.4 as i64),
                        J::Num(h.5 as i64),
                        J::Num(if h.6.is_infinite() { -1 } else { h.6 as i64 }),
                    ])
                })
                .collect();
            write(
                &mut s,
                &J::Arr(vec![J::Arr(dd), J::Arr(ml), J::Arr(sz), J::Arr(hi)]),
            );
            eprintln!("DBG {k} {s}");
        }
    }
    let docs = st.docs.clone();
    let mut final_docs = Vec::new();
    for d in &docs {
        let marks = d.get_all_marks();
        let mut ms = Vec::new();
        for m in marks {
            let m = Marker::Text(m);
            let l = st.label(&m);
            ms.push(J::Arr(vec![l, find(&m)]));
        }
        final_docs.push(J::Arr(vec![
            J::Num(d.first_line()),
            J::Num(d.line_count() as i64),
            snap(d),
            hist(d),
            J::Bool(d.is_clean(None)),
            J::Arr(ms),
        ]));
    }
    outs.push(J::Arr(final_docs));
    let marks = st.marks.clone();
    let mut fm = Vec::new();
    for m in &marks {
        let l = st.label(m);
        fm.push(J::Arr(vec![l, find(m)]));
    }
    outs.push(J::Arr(fm));
    clock::set_override(None);
    // Break linked-doc reference cycles so the test does not leak.
    for d in &docs {
        let mut others = Vec::new();
        d.iter_linked_docs(|o, _| others.push(o.clone()));
        for o in others {
            d.unlink_doc(&o);
        }
    }
    let per_op: Vec<String> = outs
        .iter()
        .map(|j| {
            let mut s = String::new();
            write(&mut s, j);
            s
        })
        .collect();
    let mut s = String::new();
    write(&mut s, &J::Arr(outs));
    (s, per_op)
}

#[test]
fn cm5_doc_ops_differential() {
    let path = std::env::var("CM_FUZZ_BASELINE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../oracle/cm5-doc-ops-baseline.json")
        });
    std::panic::set_hook(Box::new(|_| {}));
    let data: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("baseline file")).unwrap();
    let seqs = data["sequences"].as_array().unwrap();
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8)
        .clamp(8, 16);
    let chunks: Vec<&[Value]> = seqs.chunks(seqs.len().div_ceil(threads)).collect();
    let results: Vec<Vec<(i64, Option<String>)>> = std::thread::scope(|s| {
        let hs: Vec<_> = chunks
            .iter()
            .map(|chunk| {
                s.spawn(move || {
                    chunk
                        .iter()
                        .map(|seq| {
                            let seed = seq["seed"].as_i64().unwrap();
                            let expected = seq["out"].as_str().unwrap();
                            let r = catch_unwind(AssertUnwindSafe(|| replay(seq)));
                            match r {
                                Ok((got, _)) if got == expected => (seed, None),
                                Ok((_, per_op)) => {
                                    let exp = split_top_level(expected);
                                    let mut detail = String::from("whole-output mismatch");
                                    for (i, g) in per_op.iter().enumerate() {
                                        let e = exp.get(i).copied().unwrap_or("");
                                        if e != g {
                                            let op = seq["ops"]
                                                .get(i)
                                                .map(|o| o.to_string())
                                                .unwrap_or_else(|| "<final dump>".into());
                                            detail =
                                                format!("op #{i} {op}\n    {}", first_diff(e, g));
                                            break;
                                        }
                                    }
                                    (seed, Some(detail))
                                }
                                Err(e) => {
                                    (seed, Some(format!("replay panicked: {}", panic_msg(e))))
                                }
                            }
                        })
                        .collect()
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let all: Vec<(i64, Option<String>)> = results.into_iter().flatten().collect();
    let bad: Vec<&(i64, Option<String>)> = all.iter().filter(|(_, d)| d.is_some()).collect();
    println!(
        "fuzz oracle: {} sequences compared, {} mismatches",
        all.len(),
        bad.len()
    );
    for (seed, d) in bad.iter().take(15) {
        println!("seed {seed}: {}", d.as_ref().unwrap());
    }
    assert!(
        bad.is_empty(),
        "{} of {} sequences differ from CodeMirror 5",
        bad.len(),
        all.len()
    );
}

/// Split a JSON array text into its top-level element texts.
fn split_top_level(s: &str) -> Vec<&str> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    let mut start = 1;
    for (i, &c) in b.iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'[' | b'{' => depth += 1,
            b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    if i > start {
                        out.push(&s[start..i]);
                    }
                }
            }
            b',' if depth == 1 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out
}

fn first_diff(js: &str, rust: &str) -> String {
    let n = js
        .bytes()
        .zip(rust.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let mut a = n.saturating_sub(160);
    while !js.is_char_boundary(a) {
        a -= 1;
    }
    let cut = |t: &str| {
        let mut e = (n + 160).min(t.len());
        while !t.is_char_boundary(e) {
            e += 1;
        }
        let mut b = a.min(t.len());
        while !t.is_char_boundary(b) {
            b -= 1;
        }
        t[b..e].to_string()
    };
    format!(
        "(first difference at byte {n})\n    js:   …{}\n    rust: …{}",
        cut(js),
        cut(rust)
    )
}
