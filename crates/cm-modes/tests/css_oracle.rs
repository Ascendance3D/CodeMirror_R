//! C4 oracle: `mode/css/css.js` (css, scss, less, gss) must reproduce every callback
//! and every `indent()` result of `oracle/cm5-css-baseline.json`, written by
//! `node oracle/cm5-modes-tokens.mjs css` through the original `runmode.node.js`.

mod common;

#[test]
fn css_tokens_match_cm5_runmode() {
    let rep = common::check_baseline("css", &common::no_special);
    common::print_and_assert("css", &rep);
}
