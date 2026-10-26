/**
 * P10-01 oracle: mode/rust/rust.js (a defineSimpleMode table) recorded from a built
 * pinned tree. The Rust side is crates/cm-modes/tests/rust_oracle.rs.
 *
 *   node oracle/cm5-rust-mode.mjs <built-cm5-dir> [out.json]
 *        default out: crates/cm-modes/tests/data/cm5-rust-mode.json
 *
 * <built-cm5-dir> is a disposable copy of oracle/cm5-build after `npm run build`
 * (addon/runmode/runmode.node.js is a build output). Never build in oracle/cm5-build.
 *
 * Sections:
 *   rules    the states object rust.js passes to defineSimpleMode: per state, each
 *            rule's regex source and flags, token, next/push/pop/sol/indent/dedent,
 *            and the meta object;
 *   props    the mode object's properties after getMode (name, meta copies);
 *   resolve  resolveMode / getMode(...).name / mimeModes / meta.js findModeBy*;
 *   electric electricInput.test results;
 *   runs     per input line: the state before, indent(state, textAfter, line), the
 *            tokens from highlight.js's readToken loop ([text, style, start]), the
 *            state after, electricInput.test(line); a "Mode rust failed to advance
 *            stream." error ends the run. Variants: indentUnit 4 / 2, and a copy of
 *            the state (copyState) at each line start;
 *   runmode  CM.runMode callbacks ([text, style, line, start], line break ["\n"]).
 *
 * Text that is not well-formed UTF-16 is an array of code units.
 */
import { createRequire } from "node:module";
import { readFileSync, writeFileSync } from "node:fs";
import { join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const HERE  = normalize (join (fileURLToPath (import.meta.url), ".."));
const ROOT  = normalize (join (HERE, ".."));
const BUILD = process .argv [2];
if (!BUILD) { console .error ("usage: node oracle/cm5-rust-mode.mjs <built-cm5-dir> [out.json]"); process .exit (2); }
const OUT   = process .argv [3] || join (ROOT, "crates", "cm-modes", "tests", "data", "cm5-rust-mode.json");

const require = createRequire (import.meta.url);
const CM      = require (join (BUILD, "addon", "runmode", "runmode.node.js"));
CM .Pass = { toString: function () { return "CodeMirror.Pass"; } };

const enc = s => s .isWellFormed () ? s : Array .from ({ length: s .length }, (_, i) => s .charCodeAt (i));

require (join (BUILD, "addon", "mode", "simple.js"));
let STATES = null;
const define = CM .defineSimpleMode;
CM .defineSimpleMode = function (name, states) { if (name == "rust") STATES = states; return define (name, states); };
require (join (BUILD, "mode", "rust", "rust.js"));
CM .defineSimpleMode = define;
require (join (BUILD, "mode", "meta.js"));

// --- rules ----------------------------------------------------------------------

function ruleRecord (r)
{
   const out = {};
   if (r .regex instanceof RegExp) out .regex = { source: r .regex .source, flags: r .regex .flags };
   else out .regex = r .regex === undefined ? null : String (r .regex);
   out .token = r .token === undefined ? null : r .token;
   for (const k of ["next", "push", "pop", "sol", "indent", "dedent", "dedentIfLineStart", "mode"])
      if (r [k] !== undefined) out [k] = r [k];
   return out;
}

function metaValue (v)
{
   if (v instanceof RegExp) return { regexp: v .source, flags: v .flags };
   return v;
}

const rules = { states: [], meta: {} };
for (const name of Object .keys (STATES))
{
   if (name == "meta") continue;
   rules .states .push ([name, STATES [name] .map (ruleRecord)]);
}
for (const [k, v] of Object .entries (STATES .meta)) rules .meta [k] = metaValue (v);

// --- props ----------------------------------------------------------------------

const MODE4 = CM .getMode ({ indentUnit: 4 }, "rust");
const props = {
   name: MODE4 .name,
   keys: Object .keys (MODE4) .sort (),
   meta: Object .fromEntries (Object .keys (STATES .meta) .map (k => [k, metaValue (MODE4 [k])])),
   electricChars: MODE4 .electricChars ?? null,
   blankLine: typeof MODE4 .blankLine,
   helperType: MODE4 .helperType ?? null,
   innerModeAtStart: MODE4 .innerMode (CM .startState (MODE4)),
   startState: snap (CM .startState (MODE4)),
   startStateIndentUnit2: snap (CM .startState (CM .getMode ({ indentUnit: 2 }, "rust"))),
};

// --- resolve --------------------------------------------------------------------

const SPECS = ["rust", "text/x-rustsrc", "text/rust", "text/x-rust", "Rust", "rs", "application/rust",
               { name: "rust" }, { name: "text/rust", extra: 1 }, { name: "text/x-rustsrc" }, { name: "Rust" }];
const resolve = {
   specs: SPECS .map (spec => ({
      spec,
      resolved: CM .resolveMode (spec),
      modeName: CM .getMode ({}, spec) .name,
   })),
   mime: Object .fromEntries (["text/x-rustsrc", "text/rust", "text/x-rust"] .map (m => [m, CM .mimeModes [m] ?? null])),
   modeDefined: typeof CM .modes .rust,
   findModeByMIME: Object .fromEntries (["text/x-rustsrc", "text/rust"] .map (m => [m, CM .findModeByMIME (m) ?? null])),
   findModeByExtension: Object .fromEntries (["rs", "RS", "rlib"] .map (m => [m, CM .findModeByExtension (m) ?? null])),
   findModeByName: Object .fromEntries (["Rust", "rust", "rs"] .map (m => [m, CM .findModeByName (m) ?? null])),
   findModeByFileName: Object .fromEntries (["main.rs", "lib.RS", "Cargo.toml"] .map (m => [m, CM .findModeByFileName (m) ?? null])),
};

// --- electric -------------------------------------------------------------------

const ELECTRIC = ["}", "  }", "\t}", " } ", "x}", "}}", "", "{", "  )", "  }", "}\n"];
const electric = ELECTRIC .map (s => [s, MODE4 .electricInput .test (s)]);

// --- runs -----------------------------------------------------------------------

function snap (s)
{
   return {
      state: s .state,
      indent: s .indent ? s .indent .slice () : null,
      pending: s .pending ? s .pending .map (p => [p .text .length, p .token ?? null]) : null,
      stack: s .stack ? s .stack .slice () : null,
      local: s .local ? true : null,
   };
}

function readToken (mode, stream, state)   // src/line/highlight.js
{
   for (let i = 0; i < 10; i++)
   {
      const style = mode .token (stream, state);
      if (stream .pos > stream .start) return style;
   }
   throw new Error ("Mode " + mode .name + " failed to advance stream.");
}

function indentValue (v)
{
   if (v === CM .Pass) return "Pass";
   if (v === undefined) return "undefined";
   return v;
}

function run (text, spec, indentUnit, copyEachLine)
{
   const mode  = CM .getMode ({ indentUnit }, spec);
   let state   = CM .startState (mode);
   const lines = CM .splitLines (text);
   const out   = [];
   let error   = null;
   for (let i = 0; i < lines .length && !error; ++i)
   {
      const line = lines [i];
      if (copyEachLine) state = CM .copyState (mode, state);
      const before = snap (state);
      const ws     = /^\s*/ .exec (line) [0] .length;
      const indent = indentValue (mode .indent (state, line .slice (ws), line));
      const stream = new CM .StringStream (line, 4, null);
      if (line == "" && mode .blankLine) mode .blankLine (state);
      const tokens = [];
      try
      {
         while (!stream .eol ())
         {
            const style = readToken (mode, stream, state);
            tokens .push ([enc (stream .current ()), style ?? null, stream .start]);
            stream .start = stream .pos;
         }
      }
      catch (e) { error = { line: i, pos: stream .pos, message: e .message }; }
      out .push ({ before, indent, tokens, after: snap (state), electric: mode .electricInput .test (line) });
   }
   return { mode: mode .name, lines: out, error };
}

const upstreamTests = [
   "123i32\n123u32\n123_u32\n0xff_u8\n0o70_i16\n0b1111_1111_1001_0000_i32\n0usize",
   "123.0f64\n0.1f64\n0.1f32\n12E+99_f64",
   "\"foo\"\nr\"foo\"\n\"\\\"foo\\\"\"\nr#\"\"foo\"\"#\n\"foo #\\\"# bar\"\nb\"foo\"\nbr\"foo\"\nb\"\\\"foo\\\"\"\nbr#\"\"foo\"\"#\nbr##\"foo #\" bar\"##\n'h'\nb'h'",
];

const demo = readFileSync (join (BUILD, "mode", "rust", "index.html"), "utf8")
   .match (/<textarea id="code" name="code">\n?([\s\S]*?)<\/textarea>/) [1]
   .replace (/&lt;/g, "<") .replace (/&gt;/g, ">") .replace (/&amp;/g, "&");

const samples = [
   ["empty", ""],
   ["blank-lines", "\n\n\n"],
   ["whitespace-lines", "   \n\t\n  \t  \nfn a() {\n    \n}\n"],
   ["demo", demo],
   ["multi-line-string", "let s = \"abc\ndef \\\" ghi\n  jkl\";\nlet t = 1;"],
   ["string-escape-eol", "let s = \"abc\\\n    def\";\nx"],
   ["string-escapes", "\"\\n\\t\\\\\\0\\x41\\u{1F600}\" \"\\\"\" \"a\\\"b\" \"\\\\\" x"],
   ["byte-strings", "b\"bytes\\x00\" br\"raw\\n\" br#\"x\"y\"# b\"\nmulti\" end"],
   ["raw-strings", "r\"abc\" r\"a\\\" b\" r\"\nline2\nline3\" after"],
   ["raw-hash-strings", "r#\"a \"quote\" b\"# r##\"x\"#y\"## r###\"\"\"### r#\"open\n\"no\" \"#\n tail"],
   ["raw-hash-multi", "let x = r##\"\n  \"# not end\n  ## \"## done;\n"],
   ["nested-comments", "/* a /* b */ c */ d\n/* outer\n  /* inner */\n still? */ after\n"],
   ["comments", "// line comment\nx // trailing\n/* block */ y /* two\nlines */ z\n/// doc\n//! inner doc\n/** doc block */\n/*/ odd */ q"],
   ["comment-indent", "fn f() {\n    /* start\n       middle\n    }\n    end */\n    x\n}\n"],
   ["char-literals", "'a' '\\n' '\\r' '\\t' '\\0' '\\'' '\\\"' '\\\\' '\\x41' '\\u{00263A}' '\\u{1F600}' '\\u{41}' 'ab' '' '\\q'"],
   ["byte-literals", "b'a' b'\\\\' b'\\'' b'\\x7f' b'\\n' b'\\u{41}' b'ab' b'' b'\\'"],
   ["lifetimes", "fn f<'a, 'b: 'a>(x: &'a str) -> &'static str { 'outer: loop { break 'outer; } }\nimpl<'a> Foo<'a> for Bar<'_> {}"],
   ["attributes", "#[derive(Debug, Clone)]\n#![allow(dead_code)]\n#[cfg(test)] mod tests { #[test] fn t() {} }\n#[doc = \"]\"] x\n#[multi(\n  line)]\n# [spaced]\n#!\n#[unclosed"],
   ["macros", "println!(\"{}\", x);\nvec![1, 2, 3];\nmacro_rules! foo {\n    ($x:expr) => { $x };\n}\nassert_eq!(a, b); r#try!(x); x! y !z format !(\"a\")"],
   ["generics", "struct Foo<T: Clone + 'static, U = i32> where T: Debug {\n    v: Vec<Option<HashMap<String, Box<dyn Fn(u8) -> u16>>>>,\n}\nlet x = foo::<u8>(); let y: Result<Self, E> = Ok(());\nfn g<const N: usize>(a: [u8; N]) -> impl Iterator<Item = &u8> {}"],
   ["numbers", "1 1_000 0x_FF 0xFFu8 0o777 0b1010 0b102 0o78 0xg 1e10 1E10 1e+10 1e-10 1.5 1.5e3 1.5e-3f32 1. .5 1.f32 1f32 1f64 123abc 1u128 1i128 1isize 1usize 12E+99_f64 1_f32 0.0_1 1__2 1e_1 9e 0x 0b 0o 3.14_15f64 2.0e+_5"],
   ["keywords", "abstract alignof as async await box break continue const crate do dyn else enum extern fn for final if impl in loop macro match mod move offsetof override priv proc pub pure ref return self sizeof static struct super trait type typeof union unsafe unsized use virtual where while yield\nlet let mut try catch default"],
   ["keyword-boundaries", "asm ask selfish self_ self.x Self::new() Selfie iffy fnord r#type r#fn crate::x super::y as_ref matchy ifx _if if_ in_ out"],
   ["atoms-builtins", "Self isize usize char bool u8 u16 u32 u64 u128 f16 f32 f64 i8 i16 i32 i64 i128 str String Option Result true false Some None Ok Err Truer xtrue _None"],
   ["defs", "let x = 1; let mut y = 2; let\tmut\tz; let  mutt; fn main() {}\nenum E { A } mod m; struct S; type T = u8; union U {}\nlet mut\nlet\nfn\n  fn   spaced_name ()\nlet _ = 1; let 1x; fn 9() {} let mut mut x;\nconst C: u8 = 1; static S: u8 = 2; trait Tr {} impl Tr for S {}"],
   ["operators", "a -> b => c :: d && e || f != g ..= h /= i a/b a//b a/*c*/b x+=1 y<<=2 z>>3 !x -y *p &r ^q %m |o ~t ?e @a $b"],
   ["punctuation-indent", "fn f() {\n    let v = [\n        (1, 2),\n        (3, 4),\n    ];\n    if x {\n        y\n    } else {\n        z\n    }\n}\n"],
   ["dedent-text-after", "fn f() {\n    g(h(\n        i[\n            j\n        ])\n    )\n    }) ] x\n})]}\n}\n"],
   ["unbalanced", "}}}\n)\n{{{\n  {\n(((\n]\nx\n"],
   ["tabs-indent", "fn f() {\n\tif x {\n\t\ty\n\t}\n  \tz {\n\t  w\n}\n}"],
   ["closures-control", "let f = |x: i32| -> i32 { x + 1 };\nmatch v { Some(ref x) if *x > 0 => (), _ => {} }\nwhile let Some(t) = it.next() { continue; }\nfor i in 0..10 { break; }\nasync move { fut.await? }\nunsafe { *ptr }"],
   ["unicode", "let café = \"日本語 😀\";\nfn 名前() {}\nlet x = 1;\nlet　y = 2;\n  fn indented() {\n// コメント 😀\n/* 😀 */ 'é' 'ß' b'é' \" \" 𝓍 é́"],
   ["surrogates", "'😀' '\ud83d' \"\ud83d\" \ude00x 😀😀 a😀b '😀' b'😀' r\"😀\" r#\"😀\"# // 😀\n\ud800\n\udfff\n"],
   ["u2028-comment", "/* comment   more */ x"],
   ["u2028-line-comment", "// line   more\nx"],
   ["u2029-start", "x   y\nz"],
   ["incomplete", "\"unterminated\nnext line\" ok\nr#\"open\nmore\nb'\n'\nr#\nbr#x\nb\nr\nbr\n#\n/*\n*/\n/* still"],
   ["incomplete-2", "fn\nlet mut\n x\nstruct\n<\n'\n\\\n\"\n"],
   ["crlf", "fn a() {\r\n    1\r\n}\r\n\"x\r\ny\"\r"],
   ["cr", "fn a() {\r  1\r}\r"],
   ["long-line", ("let a = [" + "1, ".repeat (200) + "];") + "\n" + "/* " + "x".repeat (500) + " */"],
];

function mulberry32 (a)
{
   return function ()
   {
      a |= 0; a = a + 0x6D2B79F5 | 0;
      let t = Math .imul (a ^ a >>> 15, 1 | a);
      t = t + Math .imul (t ^ t >>> 7, 61 | t) ^ t;
      return ((t ^ t >>> 14) >>> 0) / 4294967296;
   };
}
const rnd = mulberry32 (1010);
const ri  = n => Math .floor (rnd () * n);
const ALPHA = ["fn", "let", "mut", "struct", "enum", "impl", "self", "Self", "u8", "Some", "true", "x", "_a", "foo", " ", " ", " ", "\t",
               "1", "0x1F", "1.5e3", "1_000u32", "\"", "\"", "r\"", "r#\"", "\"#", "br##\"", "\"##", "b\"", "#", "#[", "#![", "'", "'a", "'a'", "b'", "\\", "\\\"",
               "//", "/*", "*/", "{", "}", "(", ")", "[", "]", "<", ">", "::", "!", "=", "->", "=>", ";", ",", ".", "&",
               "\n", "\n", "\n", "\r\n", "é", "😀", " ", "　", "\ud83d"];
const random = [];
for (let i = 0; i < 400; ++i)
{
   let s = "";
   for (let n = ri (50); n > 0; --n) s += ALPHA [ri (ALPHA .length)];
   random .push (s);
}

const runs = [];
upstreamTests .forEach ((text, i) => runs .push ({ kind: "upstream", name: "upstream-" + i, spec: "rust", indentUnit: 4, copy: false, text: enc (text), ... run (text, "rust", 4, false) }));
for (const [name, text] of samples)
   for (const [indentUnit, copy] of [[4, false], [2, false], [4, true]])
      runs .push ({ kind: "sample", name, spec: "rust", indentUnit, copy, text: enc (text), ... run (text, "rust", indentUnit, copy) });
random .forEach ((text, i) => runs .push ({ kind: "random", name: "random-" + i, spec: "rust", indentUnit: 4, copy: i % 4 == 0, text: enc (text), ... run (text, "rust", 4, i % 4 == 0) }));
for (const spec of ["text/x-rustsrc", "text/rust", { name: "rust" }])
   runs .push ({ kind: "spec", name: "demo", spec, indentUnit: 4, copy: false, text: enc (demo), ... run (demo, spec, 4, false) });

// --- runmode --------------------------------------------------------------------

function runMode (text, spec)
{
   const toks = [];
   CM .runMode (text, spec, (t, style, line, start) =>
      toks .push (t == "\n" && line === undefined ? ["\n"] : [enc (t), style == null ? null : style, line, start]));
   return toks;
}
const runmode = [];
for (const spec of ["rust", "text/x-rustsrc", "text/rust"])
   for (const [name, text] of samples .filter (([n]) => !n .startsWith ("u2028")))
      runmode .push ({ spec, name, text: enc (text), tokens: runMode (text, spec) });

const out = { cm: "5.65.21", generator: "oracle/cm5-rust-mode.mjs", rules, props, resolve, electric, runs, runmode };
writeFileSync (OUT, JSON .stringify (out));
const nTok = runs .reduce ((n, r) => n + r .lines .reduce ((m, l) => m + l .tokens .length, 0), 0);
console .log (`${runs .length} runs (${nTok} tokens, ${runs .filter (r => r .error) .length} with errors), ${runmode .length} runMode inputs -> ${OUT}`);
