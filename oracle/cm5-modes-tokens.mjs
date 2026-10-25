/**
 * C4 oracle: tokens, styles and per-line indentation of the ported CodeMirror 5
 * modes (xml, javascript, css, htmlmixed) and the new VRML mode, all run through the
 * original addon/runmode/runmode.node.js. The Rust side
 * (crates/cm-modes/tests/<key>_oracle.rs) must reproduce every callback exactly:
 * text, style, line, start; and every indent() result.
 *
 *   node oracle/cm5-modes-tokens.mjs <key> [out.json]
 *        key = xml | javascript | css | htmlmixed | vrml
 *        default out: oracle/cm5-<key>-baseline.json
 *   node oracle/cm5-modes-tokens.mjs --dump <spec-json> <file>
 *        prints the canonical token/indent records of one file (for diffing
 *        against `cargo run --example cm_dump`).
 *
 * Inputs come from oracle/cm5-modes-inputs/<key>.mjs: hand-written samples, seeded
 * random documents, and real files (read by path, gzip decompressed in memory, UTF-8
 * decoded with the BOM kept and U+FFFD for bad bytes). Samples and random cases are
 * stored in full; files are stored as SHA-256 digests of canonical records (the
 * corpus is ~60 MB), with callback and line counts.
 *
 * Indentation: before each line (before blankLine and the first token), as
 * test/mode_test.js does, `mode.indent(state, line minus leading \s, line)` is
 * recorded: a number, "NaN", "Pass", "string:<value>" or "other:<typeof>". runmode.node.js has no
 * CodeMirror.Pass; it is defined here as in lib/codemirror.js so the modes return the
 * same object they return in a browser.
 */
import { createRequire } from "node:module";
import { readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { gunzipSync } from "node:zlib";
import { join, normalize, relative } from "node:path";
import { fileURLToPath } from "node:url";

const HERE    = normalize (join (fileURLToPath (import.meta.url), ".."));
const ROOT    = normalize (join (HERE, ".."));
const require = createRequire (import.meta.url);
const CM      = require (join (HERE, "cm5-build", "addon", "runmode", "runmode.node.js"));

CM .Pass = { toString: function () { return "CodeMirror.Pass"; } };

// Same order as test/index.html: htmlmixed after xml, so text/html -> htmlmixed.
for (const m of ["toml/toml.js", "xml/xml.js", "javascript/javascript.js", "css/css.js", "htmlmixed/htmlmixed.js"])
   require (join (HERE, "cm5-build", "mode", m));
// The VRML mode is plain CM5 mode source outside the CM tree: run its UMD wrapper in
// "browser" form with this CodeMirror.
new Function ("CodeMirror", "exports", "module", "define", readFileSync (join (HERE, "cm5-vrml-mode.js"), "utf8")) (CM);

export const enc = s => s .isWellFormed () ? s : Array .from ({ length: s .length }, (_, i) => s .charCodeAt (i));

// Spec objects that need values JSON cannot carry (regexps) are named "@id".
const SPECIAL = {
   "@htmlmixed-scripttypes": () => ({
      name: "htmlmixed",
      scriptTypes: [{ matches: /\/x-handlebars-template|\/x-mustache/i, mode: null },
                    { matches: /(text|application)\/(x-)?vb(a|script)/i, mode: "vbscript" }],
   }),
   "@htmlmixed-tags": () => ({
      name: "htmlmixed",
      tags: { style: [["type", /^text\/(x-)?scss$/, "text/x-scss"], [null, null, "css"]],
              custom: [[null, null, "text/x-toml"]] },
   }),
};
export const specValue = spec => typeof spec == "string" && spec .startsWith ("@") ? SPECIAL [spec] () : spec;

export function indentValue (r)
{
   if (typeof r == "number") return Number .isNaN (r) ? "NaN" : String (r);
   if (r === CM .Pass) return "Pass";
   // "CodeMirror.Pass" + n: a Pass base indent (htmlmixed, inside <pre>) plus a number.
   if (typeof r == "string") return "string:" + r;
   return "other:" + typeof r;
}

/** runMode with the indent hook. Returns {tokens, indents, error}. */
export function run (text, spec)
{
   const tokens = [], indents = [];
   const mode   = CM .getMode (CM .defaults, specValue (spec));
   const lines  = CM .splitLines (text);
   let error    = null;
   try
   {
      const state = CM .startState (mode);
      for (let i = 0; i < lines .length; ++i)
      {
         if (i) tokens .push (["\n"]);
         if (mode .indent)
         {
            const ws = lines [i] .match (/^\s*/) [0];
            indents .push (indentValue (mode .indent (state, lines [i] .slice (ws .length), lines [i])));
         }
         const stream = new CM .StringStream (lines [i], null, {
            lookAhead: n => lines [i + n],
            baseToken: function () {},
         });
         if (!stream .string && mode .blankLine) mode .blankLine (state);
         let guard = 0;
         while (!stream .eol ())
         {
            const style = mode .token (stream, state);
            tokens .push ([enc (stream .current ()), style == null ? null : style, i, stream .start]);
            if (stream .pos == stream .start && ++guard > 1000) throw new Error ("runMode spins: no progress");
            stream .start = stream .pos;
         }
      }
   }
   catch (e)
   {
      error = String (e && e .message || e);
   }
   return { tokens, indents: mode .indent ? indents : null, error };
}

/** Canonical records, hashed: see crates/cm-modes/tests/common/mod.rs. */
export function digest ({ tokens, indents })
{
   const th = createHash ("sha256"), ih = createHash ("sha256");
   for (const t of tokens)
   {
      if (t .length == 1) { th .update (Buffer .from ("\n", "utf16le")); continue; }
      const text = typeof t [0] == "string" ? t [0] : String .fromCharCode (... t [0]);
      th .update (Buffer .from (`${t [2]},${t [3]},${text .length},${t [1] == null ? "\u0000" : t [1]}|${text}`, "utf16le"));
   }
   for (let i = 0; indents && i < indents .length; ++i)
      ih .update (Buffer .from (`${i}:${indents [i]}\n`, "utf16le"));
   return { tokenHash: th .digest ("hex"), indentHash: indents ? ih .digest ("hex") : null };
}

export function readText (path)
{
   let bytes = readFileSync (path);
   const gz  = bytes .length >= 2 && bytes [0] == 0x1f && bytes [1] == 0x8b;
   if (gz) bytes = gunzipSync (bytes);
   return { gz, text: new TextDecoder ("utf-8", { ignoreBOM: true }) .decode (bytes) };
}

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

async function main ()
{
   if (process .argv [2] == "--dump")
   {
      const spec = JSON .parse (process .argv [3]);
      const r    = run (readText (process .argv [4]) .text, spec);
      for (const t of r .tokens)
         console .log (t .length == 1 ? "\\n" : JSON .stringify (t));
      (r .indents || []) .forEach ((v, i) => console .log (`indent ${i}:${v}`));
      if (r .error) console .log ("error " + r .error);
      return;
   }
   const key = process .argv [2];
   const OUT = process .argv [3] || join (HERE, `cm5-${key}-baseline.json`);
   const inp = (await import (`./cm5-modes-inputs/${key}.mjs`)) .default;

   const cases = [];
   for (const { spec, text, kind } of inp .samples)
      cases .push ({ kind: kind || "sample", spec, text: enc (text), ... run (text, spec) });

   for (const r of inp .random || [])
   {
      const rnd = mulberry32 (r .seed), ri = n => Math .floor (rnd () * n);
      for (let i = 0; i < r .count; ++i)
      {
         let s = "";
         for (let n = ri (r .maxLen); n > 0; --n) s += r .alphabet [ri (r .alphabet .length)];
         const spec = r .specs [i % r .specs .length];
         cases .push ({ kind: "random", spec, text: enc (s), ... run (s, spec) });
      }
   }

   const files = [];
   for (const { spec, paths } of inp .files || [])
   {
      for (const abs of paths)
      {
         const { gz, text } = readText (abs);
         const r = run (text, spec);
         files .push ({ path: relative (ROOT, abs), spec, gz, units: text .length,
                        lines: CM .splitLines (text) .length, callbacks: r .tokens .length,
                        ... digest (r), error: r .error });
      }
   }

   for (const c of cases) if (c .error) console .log (`JS error in ${c .kind} case: ${c .error}`);
   for (const f of files) if (f .error) console .log (`JS error in ${f .path}: ${f .error}`);
   writeFileSync (OUT, JSON .stringify ({ cm: "5.65.21", generator: "oracle/cm5-modes-tokens.mjs " + key, cases, files }));
   const cb = cases .reduce ((n, c) => n + c .tokens .length, 0) + files .reduce ((n, f) => n + f .callbacks, 0);
   const il = cases .reduce ((n, c) => n + (c .indents ? c .indents .length : 0), 0) +
              files .reduce ((n, f) => n + (f .indentHash ? f .lines : 0), 0);
   console .log (`${key}: ${cases .length} cases + ${files .length} files, ${cb} callbacks, ${il} indent lines -> ${relative (ROOT, OUT)}`);
}

if (process .argv [1] && normalize (process .argv [1]) == fileURLToPath (import.meta.url))
   await main ();
