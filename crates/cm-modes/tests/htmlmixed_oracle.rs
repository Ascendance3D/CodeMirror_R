//! C4 oracle for `mode/htmlmixed/htmlmixed.js` (with xml, javascript and css nested):
//! every runMode callback and indent() of `oracle/cm5-htmlmixed-baseline.json`
//! (from `node oracle/cm5-modes-tokens.mjs htmlmixed`).

mod common;

use std::rc::Rc;

use cm_modes::htmlmixed::{HtmlTags, ScriptType, TagRule, contains_ci, obj};
use cm_modes::{ModeSpec, Spec};

/// The `@id` specs of `oracle/cm5-modes-tokens.mjs` (`SPECIAL`), with their regexps
/// as predicates.
fn special(id: &str) -> Spec {
    match id {
        // scriptTypes: [{matches: /\/x-handlebars-template|\/x-mustache/i, mode: null},
        //               {matches: /(text|application)\/(x-)?vb(a|script)/i, mode: "vbscript"}]
        "@htmlmixed-scripttypes" => {
            let st = vec![
                ScriptType {
                    matches: Rc::new(|s: &[u16]| {
                        contains_ci(s, "/x-handlebars-template") || contains_ci(s, "/x-mustache")
                    }),
                    mode: Spec::Null,
                },
                ScriptType {
                    matches: Rc::new(|s: &[u16]| {
                        ["text/", "application/"].iter().any(|a| {
                            ["", "x-"].iter().any(|x| {
                                ["vba", "vbscript"]
                                    .iter()
                                    .any(|v| contains_ci(s, &format!("{a}{x}{v}")))
                            })
                        })
                    }),
                    mode: Spec::from("vbscript"),
                },
            ];
            Spec::Obj(ModeSpec::named("htmlmixed").with("scriptTypes", obj(st)))
        }
        // tags: {style: [["type", /^text\/(x-)?scss$/, "text/x-scss"], [null, null, "css"]],
        //        custom: [[null, null, "text/x-toml"]]}
        "@htmlmixed-tags" => {
            let tags: HtmlTags = vec![
                (
                    "style".into(),
                    vec![
                        TagRule {
                            attr: Some("type".into()),
                            matches: Some(Rc::new(|s: &[u16]| {
                                s == cm_core::text::to_u16("text/scss")
                                    || s == cm_core::text::to_u16("text/x-scss")
                            })),
                            mode: Spec::from("text/x-scss"),
                        },
                        TagRule {
                            attr: None,
                            matches: None,
                            mode: Spec::from("css"),
                        },
                    ],
                ),
                (
                    "custom".into(),
                    vec![TagRule {
                        attr: None,
                        matches: None,
                        mode: Spec::from("text/x-toml"),
                    }],
                ),
            ];
            Spec::Obj(ModeSpec::named("htmlmixed").with("tags", obj(tags)))
        }
        _ => panic!("unknown special spec {id}"),
    }
}

#[test]
fn htmlmixed_tokens_match_cm5_runmode() {
    let rep = common::check_baseline("htmlmixed", &special);
    common::print_and_assert("htmlmixed", &rep);
}
