/**
 * Writes crates/cm-modes/tests/data/mode-infra-cases.json: the inputs of the P10-00
 * oracle (oracle/cm5-mode-infra.mjs) and its Rust twin (mode_infra_oracle.rs).
 * Strings that are not well-formed UTF-16 are stored as arrays of code units.
 *
 *   node oracle/cm5-mode-infra-cases.mjs [out.json]
 */

import { writeFileSync } from "node:fs";
import { join, normalize } from "node:path";
import { fileURLToPath } from "node:url";
const ROOT = normalize (join (fileURLToPath (import.meta.url), "..", ".."));
const enc = s => s .isWellFormed () ? s : Array .from ({ length: s .length }, (_, i) => s .charCodeAt (i));
const texts = {
  simple: [
    'function foo(a, b) {',
    '  var x = "str\\"ing" + 0x1F + 1.5e3; // note',
    '  if (x) { return @obj.prop [[key]] %ab-cd %-x %; }',
    '  /* multi',
    '     line */ ^up! ^down! #TAGS.X ABC Abc',
    '}',
    '<<here',
    'doc >> after',
    '~~ sol ~~',
    '<script>var s = 1; // c</script> tail',
    '<style>a { color: red }</style><style>b{}',
    '</style>',
    '<raw>let r = 2 </raw> after raw',
    'é éé\u{1F600} x\uD800y \uDC00z',
    '',
    '\ttabbed {',
    '\t}',
    '  unclosed "str',
    '  ( [ {',
    '  } ] )',
  ].join("\n"),
  overlay: [
    '// TODO fix this TODO',
    'var TODO = 1, xTODOy = "TODO";',
    '',
    'TODO',
    '<b>hello TODO</b> x',
    '  \u{1F600}TODO\uD800',
  ].join("\n"),
  multi: [
    '<div class="a">{{ a + b }}</div>',
    '<p><%= color: red %> and <% x %></p>',
    '[[ foo(1) ]] tail [[ (',
    ') ]]',
    '!! var z = 1;',
    'next line',
    '{{ unterminated',
    'still js }} done',
    '',
    '<%',
    '',
    'a { b: c } %>',
  ].join("\n"),
  spin: 'a &b c & d',
  newline: [
    'var a = 1;',
    '',
    'body { color: red }',
    '',
    'var b = {',
    '',
    '',
    '  c: 2 };',
  ].join("\n"),
};
const all = Object .keys (texts);
const runs = [
  { id: "simple", spec: "p10simple", texts: all, indentUnits: [2, 4] },
  { id: "simple-mime", spec: "text/x-p10simple", texts: ["simple"] },
  { id: "overlay", spec: { name: "p10overlay" }, texts: all },
  { id: "overlay-combine", spec: { name: "p10overlay", combine: true }, texts: all },
  { id: "overlay-combinetokens", spec: { name: "p10overlay", combine2: true }, texts: all },
  { id: "overlay-combinetokens-combine", spec: { name: "p10overlay", combine2: true, combine: true }, texts: ["overlay"] },
  { id: "overlay-html", spec: { name: "p10overlay", backdrop: "text/html", combine: true }, texts: all },
  { id: "multi", spec: "p10multi", texts: all },
  { id: "multinl", spec: "p10multinl", texts: all },
];
const R = (s, f, i, from = 0) => [enc (s), f, enc (i), from];
const regexp = [
  R ("a+b", "", "xaaab"), R ("^(?:a|ab)c", "", "abc"), R ("(a)|(b)", "", "b"), R ("a*?b", "", "aab"),
  R ("<.*?>", "", "<a><b>"), R ("x{2,3}", "", "xxxx"), R ("x{2,}?", "", "xxxx"), R ("a{,2}", "", "a{,2}"),
  R ("\\bfoo\\b", "", "a foo."), R ("foo(?=bar)", "", "foobaz foobar"), R ("foo(?!bar)", "", "foobar foobaz"),
  R ("(?<=\\$)\\d+", "", "a1 $42"), R ("(?<!\\$)\\b\\d+", "", "$4 5"), R ("(a)\\1", "", "xaa"),
  R ("(?<q>['\"]).*?\\k<q>", "", "x'a\"b'"), R ("[^\\s\\]]+", "", "  ab]"), R ("[\\w-]+", "", "a-b c"),
  R ("\\x41\\u0042\\103", "", "ABC"), R ("^$", "m", "a\n\nb"), R ("a.c", "", "a\nc abc"), R ("a.c", "s", "a\nc"),
  R ("(?:(a)|b)+", "", "ab"), R ("(a*)*b", "", "b"), R ("(a|)*", "", "aa"), R ("(a|ab)(c|bcd)(d*)", "", "abcd"),
  R ("ABC", "i", "xabc"), R ("[a-z]+", "i", "XyZ1"), R ("[^a-z]", "i", "Q1"), R ("é", "i", "É"), R ("s", "i", "ſ"),
  R ("ſ", "i", "S"), R ("k", "i", "K"), R ("K", "iu", "k"), R ("(a)\\1", "i", "aA"), R ("[\\u00e0-\\u00ff]+", "i", "ÀÉx"),
  R ("^.", "", "\u{1F600}x"), R ("^.", "u", "\u{1F600}x"), R ("\\u{1F600}", "u", "a\u{1F600}"), R ("[\\uD83D]", "", "\u{1F600}"),
  R ("[\\uD83D]", "u", "\u{1F600}"), R ("^[^x]", "u", "\u{1F600}"), R ("x", "g", "\u{1F600}x", 1), R ("x", "", "x x", 2),
  R ("x", "y", "ax", 0), R ("x", "y", "ax", 1), R ("\\uD800", "", "a\uD800b"), R ("\\uD83D\\uDE00", "u", "\u{1F600}"),
  R ("\\8", "", "8"), R ("a]", "", "a]"), R ("\\c1", "", "\\c1"), R ("\\cJ", "", "\n"), R ("[\\c1]", "", "\x11"),
  R ("(?=a)*a", "", "a"), R ("\\0", "", "\0"), R ("\\07", "", "\x07"), R ("\\377", "", "\xff"), R ("\\400", "", "\x200"),
  R ("[\\b]", "", "\b"), R ("[\\d-z]+", "", "1-z"), R ("[--0]", "", "/"), R ("\\s+", "", "a  ﻿b"),
  R ("\\w+", "", "αβ_x1"), R ("\\W", "", "aé"), R ("[^]", "", "\n"), R ("[]", "", "a"), R ("$|a", "", "ba"),
  R ("(?:)", "", "abc"), R ("^(?:)", "", ""), R ("a{2}{3}", "", "a"), R ("a**", "", "a"), R ("(", "", "a"), R ("a)", "", "a"),
  R ("[b-a]", "", "a"), R ("x{3,2}", "", "x"), R ("\\k<n>", "u", "k"), R ("\\k<n>", "", "k<n>"), R ("a{", "u", "a{"),
  R ("(?<=a)*", "", "a"), R ("a", "gg", "a"), R ("\\p{L}", "u", "a"), R ("\\p", "", "p"), R ("(?<n>a)(?<n>b)", "", "ab"),
  R ("[\\w-a]", "", "-"), R ("[\\w-a]", "u", "-"), R ("\\-", "", "-"), R ("\\-", "u", "-"), R ("\\/", "u", "/"),
  R ("(\\d+)\\s*(?:px|em)?", "", "  12 em"), R ("^(?:\\/\\/.*)", "", "// x"), R ("(?:\\d*\\.)?\\d+(?:e[-+]?\\d+)?", "i", "x .5E-3"),
  R ("^(?:[^\\\\]|\\\\.)*?(?:\"|$)", "", "a\\\"b\"c"), R ("<(\\/?)([\\w-]+)", "", "</my-tag>"),
];
const meta = {
  mime: ["text/x-c++src", "TEXT/HTML", "application/pgp-keys", "image/svg+xml", "application/foo+json", "text/nope", "text/x-gfm", "text/javascript", "application/ld+json", "text/x-sh"],
  ext: ["rs", "RS", "build", "BUILD", "js", "h", "c++", "cmake.in", "", "md", "nope"],
  file: ["CMakeLists.txt", "cmakelists.txt", "README.md", "readme.MD", "my-nginx-site.conf", "a.b.py", "noext", "trailing.", "Dockerfile", "PKGBUILD", "BUCK", "Jenkinsfile", "extensions.conf", ".bashrc", "x.tar.gz"],
  name: ["C++", "cpp", "pgp", "PGP", "javascript", "JS", "rust", "Shell", "bash", "nope", "HTML", "xhtml"],
};
const resolve = [
  "text/x-toml", "text/plain", "toml", "nope", "image/svg+xml", "application/foo+json", "application/ld+json",
  "text/html", "text/javascript", "application/json", "text/x-scss", "text/x-p10simple", "p10simple",
  { name: "text/x-toml", x: true }, { name: "javascript", json: true }, { name: "text/typescript", extra: "y" },
  { name: "application/json", json: false }, { name: "nope" },
];
writeFileSync (process .argv [2] || join (ROOT, "crates", "cm-modes", "tests", "data", "mode-infra-cases.json"), JSON .stringify ({ texts: Object .fromEntries (Object .entries (texts) .map (([k, v]) => [k, enc (v)])), runs, regexp, meta, resolve }, null, 1) + "\n");
