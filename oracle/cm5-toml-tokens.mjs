/**
 * C3 oracle: tokens and styles of mode/toml via addon/runmode/runmode.node.js.
 * crates/cm-modes/tests/toml_oracle.rs must reproduce every callback exactly.
 *
 *   node oracle/cm5-toml-tokens.mjs [out.json]    (default oracle/cm5-toml-baseline.json)
 *
 * Inputs: hand-written samples (edge cases included) plus seeded random documents
 * over a TOML-ish alphabet. U+2028/U+2029 are excluded: inside a string the mode's
 * `/^.[^\\\"\']*\/` never matches them and runMode spins forever.
 *
 * Each callback becomes [text, style, line, start]; the line-break callback is
 * ["\n"]. Token text that is not well-formed UTF-16 is emitted as an array of code
 * units (lone surrogates do not survive JSON parsers).
 */
import { createRequire } from "node:module";
import { writeFile } from "node:fs/promises";
import { join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const HERE    = normalize (join (fileURLToPath (import.meta.url), ".."));
const require = createRequire (import.meta.url);
const CM      = require (join (HERE, "cm5-build", "addon", "runmode", "runmode.node.js"));
require (join (HERE, "cm5-build", "mode", "toml", "toml.js"));

const OUT = process .argv [2] || join (HERE, "cm5-toml-baseline.json");

const enc = s => s .isWellFormed () ? s : Array .from ({ length: s .length }, (_, i) => s .charCodeAt (i));

function run (text, spec)
{
   const toks = [];
   CM .runMode (text, spec, (t, style, line, start) =>
   {
      toks .push (t == "\n" && line === undefined ? ["\n"] : [enc (t), style == null ? null : style, line, start]);
   });
   return toks;
}

const samples = [
   "",
   "\n",
   "# This is a TOML document.\n\ntitle = \"TOML Example\"\n",
   "[owner]\nname = \"Tom Preston-Werner\"\ndob = 1979-05-27T07:32:00-08:00 # First class dates\n",
   "[database]\nserver = \"192.168.1.1\"\nports = [ 8000, 8001, 8002 ]\nconnection_max = 5000\nenabled = true\n",
   "[servers]\n\n  # Indentation (tabs and/or spaces) is allowed but not required\n  [servers.alpha]\n  ip = \"10.0.0.1\"\n  dc = \"eqdc10\"\n",
   "[clients]\ndata = [ [\"gamma\", \"delta\"], [1, 2] ]\n\n# Line breaks are OK when inside arrays\nhosts = [\n  \"alpha\",\n  \"omega\"\n]\n",
   "[[products]]\nname = \"Hammer\"\nsku = 738594937\n\n[[products]]\n\n[[products]]\nname = \"Nail\"\nsku = 284758393\ncolor = \"gray\"\n",
   "date = 1979-05-27T07:32:00Z\nneg = -17\nflt = -3.1415\nbad = 3.\nexp = 5e+22\nyes = trueish\nno = falsehood\n",
   "str = \"I'm a string. \\\"You can quote me\\\". Name\\tJos\\u00E9\\nLocation\\tSF.\"\n",
   "lit = 'C:\\Users\\nodejs\\templates'\nmixed = \"a'b\" 'c\"d'\n",
   "unterminated = \"abc\ncontinues = here\" after\nnext = 1\n",
   "\"quoted key\" = 1\n'single' = 2\nkey with spaces = 3\n=4\n",
   "a = [ [ 1, 2 ], [3,\n4 ] ]\nb = ]\n]]\nc = [\n[\n",
   "trailing \\\nbackslash = \"x\\\\\"\nend = \"\\\n",
   "\t\ttabbed\t=\t\"t\"\t# comment\t\n",
   "crlf = 1\r\nnext = 2\r\n\r\nlast = \"x\"\r",
   "cr = 1\rnext = 2\r\rx",
   "uni = \"日本語 😀 \u00a0nbsp\"\n😀key = 1\nk\u00a0= 2\n",
   "lone = \"\ud83d\"\n\ude00 = 1\n",
   "[table\nnot closed\n[ok] trailing\n[[double]] x\n[a]]]\n",
   "#\n# \n  #x\n[x]#y\n",
   "v = 1979-05-27\nw = 1979-05-27Z\nx = 12345Z\ny = 0000T::..Z\nz = 1979-05-27T00:00:00.999Z!",
   "x = \"a\\\"b\"c\nd = '\\''\ne = \"\\\\\"\n",
   "=\n==\n= =\n a = b = c\n",
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
const rnd = mulberry32 (8080);
const ri  = n => Math .floor (rnd () * n);
const ALPHA = ["a", "b", "k", " ", " ", "\t", "=", "=", "\"", "'", "\\", "[", "]", "[[", "]]", "#", "1", "2", "-", ".", "T", "Z", ":",
               "true", "false", "1979-05-27T07:32:00Z", "-3.5", "\n", "\n", "\n", "\r\n", "\r", ",", "é", "😀", "\u00a0", "\u3000"];
const random = [];
for (let i = 0; i < 400; ++i)
{
   let s = "";
   for (let n = ri (40); n > 0; --n) s += ALPHA [ri (ALPHA .length)];
   random .push (s);
}

const cases = [];
for (const text of samples) cases .push ({ kind: "sample", spec: "text/x-toml", text: enc (text), tokens: run (text, "text/x-toml") });
for (const text of random)  cases .push ({ kind: "random", spec: "toml", text: enc (text), tokens: run (text, "toml") });
// Mode resolution through runMode: plain text and an unknown mode both use "null".
for (const spec of ["text/plain", "no-such-mode"])
   cases .push ({ kind: "fallback", spec, text: samples [3], tokens: run (samples [3], spec) });

await writeFile (OUT, JSON .stringify ({ cm: "5.65.21", generator: "oracle/cm5-toml-tokens.mjs", cases }));
console .log (`${cases .length} inputs, ${cases .reduce ((n, c) => n + c .tokens .length, 0)} callbacks -> ${OUT}`);
