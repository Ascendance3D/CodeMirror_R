//! C4 oracle: `mode/javascript` (javascript, json, json-ld, typescript) must
//! reproduce `oracle/cm5-javascript-baseline.json` (from
//! `node oracle/cm5-modes-tokens.mjs javascript`): every runMode callback (text,
//! style, line, start) and every per-line `indent()` result.

mod common;

#[test]
fn javascript_tokens_match_cm5_runmode() {
    let rep = common::check_baseline("javascript", &common::no_special);
    common::print_and_assert("javascript", &rep);
}
