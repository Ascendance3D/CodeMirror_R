//! C4 oracle for `mode/xml/xml.js`: every runMode callback and every indent() of
//! `oracle/cm5-xml-baseline.json` (from `node oracle/cm5-modes-tokens.mjs xml`).

mod common;

#[test]
fn xml_tokens_match_cm5_runmode() {
    let rep = common::check_baseline("xml", &common::no_special);
    common::print_and_assert("xml", &rep);
}
