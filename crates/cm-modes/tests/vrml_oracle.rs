//! C4: the new VRML mode, JS (`oracle/cm5-vrml-mode.js`, run by CM5's runmode.node.js)
//! against Rust (`src/vrml.rs`): every callback and indent() of
//! `oracle/cm5-vrml-baseline.json` (from `node oracle/cm5-modes-tokens.mjs vrml`),
//! including all 606 `.wrl` files of `oracle/corpus-items`.

mod common;

#[test]
fn vrml_tokens_match_js_vrml_mode() {
    let rep = common::check_baseline("vrml", &common::no_special);
    common::print_and_assert("vrml", &rep);
}
