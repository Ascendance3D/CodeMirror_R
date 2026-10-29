//! `findWordAt` with the `wordChars` helper against `tests/data/cm5-find-word-at.json`
//! (recorded by `oracle/cm5-find-word-at.mjs`; Chromium, Firefox and WebKit agree).
//! Pinned CM5 src/edit/methods.js:335-350 and src/util/misc.js:129-133: the word branch
//! uses the helper, the punctuation branch tests `isWordChar(ch)` without it.

use std::path::PathBuf;

use cm_core::{Pos, Sticky};
use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, Metrics};
use serde_json::Value;

#[test]
fn find_word_at_matches_cm5() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/cm5-find-word-at.json");
    let d: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("cm5-find-word-at.json"))
            .unwrap();
    assert_eq!(d["version"], "5.65.21");
    let cases = d["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 372);
    let metrics = Metrics {
        char_width: 10.0,
        text_height: 15.0,
        padding_h: 0.0,
        padding_top: 0.0,
        wide_width: 20.0,
        astral_width: 20.0,
    };
    let mut fails = Vec::new();
    for c in cases {
        let text = c["text"].as_str().unwrap();
        let mode = c["mode"].as_str().unwrap_or("null");
        let e = Editor::new(
            text,
            Spec::from(mode),
            EditorOptions::default(),
            metrics.clone(),
        );
        let sticky = match c["sticky"].as_str() {
            Some("before") => Some(Sticky::Before),
            Some("after") => Some(Sticky::After),
            _ => None,
        };
        let ch = c["ch"].as_i64().unwrap();
        let (from, to) = e.find_word_at(Pos::with_sticky(0, ch, sticky));
        let want = (c["from"].as_i64().unwrap(), c["to"].as_i64().unwrap());
        if (from.ch, to.ch) != want {
            fails.push(format!(
                "{mode} {text:?} ch {ch} {sticky:?}: got {:?} want {want:?}",
                (from.ch, to.ch)
            ));
        }
    }
    assert!(
        fails.is_empty(),
        "{} of {} differ:\n{}",
        fails.len(),
        cases.len(),
        fails.join("\n")
    );
}
