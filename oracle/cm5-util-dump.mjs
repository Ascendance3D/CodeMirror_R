/**
 * C1 oracle: dump the pure helpers of CodeMirror 5 (src/util/misc.js, bidi.js,
 * StringStream.js) on exhaustive and seeded random inputs.
 * crates/cm-core/tests/util_oracle.rs compares the Rust port against the output.
 *
 *   node oracle/cm5-util-dump.mjs [out.json]      (default oracle/cm5-util-baseline.json)
 *
 * Node only (the modules are DOM-free). Reads oracle/cm5-build/src, never writes it.
 */
import { writeFile } from "node:fs/promises";
import { join, normalize } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = normalize (join (fileURLToPath (import.meta.url), ".."));
const SRC  = join (HERE, "cm5-build", "src", "util");
const misc   = await import (pathToFileURL (join (SRC, "misc.js")));
const bidi   = await import (pathToFileURL (join (SRC, "bidi.js")));
const { default: StringStream } = await import (pathToFileURL (join (SRC, "StringStream.js")));

const OUT = process .argv [2] || join (HERE, "cm5-util-baseline.json");

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
const rnd  = mulberry32 (20261011);
const ri   = n => Math .floor (rnd () * n);
const pick = a => a [ri (a .length)];
const str  = (alphabet, max) => { let s = ""; for (let n = ri (max + 1); n > 0; --n) s += pick (alphabet); return s; };

// Exhaustive per-code-unit tables.
let wordChar = "", extending = "";
for (let c = 0; c < 0x10000; ++c)
{
   const ch = String .fromCharCode (c);
   wordChar  += misc .isWordCharBasic (ch) ? "1" : "0";
   extending += misc .isExtendingChar (ch) ? "1" : "0";
}

const WS = [" ", " ", "\t", "\t", "a", "b", " ", "　", "x"];
const countColumn = [], findColumn = [];
for (let i = 0; i < 3000; ++i)
{
   const s = str (WS, 14), tab = 1 + ri (8);
   const end = ri (4) == 0 ? null : ri (s .length + 1);
   const startIndex = ri (3) == 0 ? ri ((end ?? s .length) + 1) : 0;
   const startValue = startIndex ? misc .countColumn (s, startIndex, tab) : 0;
   countColumn .push ([s, end, tab, startIndex, startValue, misc .countColumn (s, end, tab, startIndex, startValue)]);
   const goal = ri (30);
   findColumn .push ([s, goal, tab, misc .findColumn (s, goal, tab)]);
}

const BIDI = ["a", "b", " ", " ", "1", "2", "+", "-", ",", ".", "%", "$", "א", "ב", "ש", "ا", "ل",
              "١", "٢", "ً", "̀", "‌", " ", "\t", "(", ")", "!", "܀", "ࢠ", "۵"];
const order = [];
for (let i = 0; i < 4000; ++i)
{
   const s = str (BIDI, 16), dir = ri (2) ? "ltr" : "rtl";
   const o = bidi .getOrder ({ text: s }, dir);
   const plain = o ? o .map (p => [p .level, p .from, p .to]) : false;
   const parts = [];
   if (o)
   {
      for (let k = 0; k < 3; ++k)
      {
         const ch = ri (s .length + 1), sticky = ri (2) ? "before" : "after";
         const r = bidi .getBidiPartAt (o, ch, sticky);
         parts .push ([ch, sticky, r == null ? null : r, bidi .bidiOther == null ? null : bidi .bidiOther]);
      }
   }
   const from = ri (s .length + 1), to = from + ri (s .length + 1 - from), secs = [];
   bidi .iterateBidiSections (o || null, from, to, (a, b, d, idx) => secs .push ([a, b, d, idx === undefined ? null : idx]));
   order .push ([s, dir, plain, parts, [from, to, secs]]);
}

const EXT = ["a", "́", "̀", "b", "ְ", "‍", "\ud83d", "\ude00"];
const skipExtending = [];
for (let i = 0; i < 1000; ++i)
{
   const s = str (EXT, 8), pos = ri (s .length + 1), dir = ri (2) ? 1 : -1;
   skipExtending .push ([Array .from ({ length: s .length }, (_, i) => s .charCodeAt (i)), pos, dir, misc .skipExtendingChars (s, pos, dir)]);
}

const findFirst = [];
for (let i = 0; i < 1000; ++i)
{
   const a = ri (40), b = ri (40), threshold = ri (40);
   // pred must hold at `to`: use a monotone predicate oriented by direction.
   const pred = a > b ? x => x <= Math .max (threshold, b) : x => x >= Math .min (threshold, b);
   findFirst .push ([a, b, threshold, misc .findFirst (pred, a, b)]);
}

// StringStream: random scripts of cursor operations on random lines.
const SS = ["a", "b", " ", "\t", " ", "#", "=", "\"", "\\", "é", "1", "x"];
const OPS = ["next", "peek", "eol", "sol", "eatSpace", "skipToEnd", "backUp", "column", "indentation", "eat", "eatWhile", "skipTo", "matchStr", "matchStrCI", "current", "start"];
const streams = [];
for (let i = 0; i < 1500; ++i)
{
   const line = str (SS, 12), tab = ri (3) ? 4 : null;
   const lineStart = ri (4) == 0 ? ri (line .length + 1) : 0;
   const s = new StringStream (line, tab);
   s .lineStart = lineStart;
   s .pos = s .start = lineStart;
   const script = [];
   for (let k = 0; k < 12; ++k)
   {
      const op = pick (OPS);
      let arg = pick (SS), flag = null;
      if (op == "matchStr") { arg = arg + pick (SS); flag = ri (2) == 0; }
      if (op == "matchStrCI") arg = pick (["A", "b", "X", "É", "é1", "B="]);
      let r;
      switch (op)
      {
         case "next": r = s .next (); break;
         case "peek": r = s .peek (); break;
         case "eol": r = s .eol (); break;
         case "sol": r = s .sol (); break;
         case "eatSpace": r = s .eatSpace (); break;
         case "skipToEnd": s .skipToEnd (); break;
         case "backUp": if (s .pos > s .start) s .backUp (1); break;
         case "column": r = s .column (); break;
         case "indentation": r = s .indentation (); break;
         case "eat": r = s .eat (arg); break;
         case "eatWhile": r = s .eatWhile (arg); break;
         case "skipTo": r = s .skipTo (arg); break;
         case "matchStr": r = s .match (arg, flag); break;
         case "matchStrCI": r = s .match (arg, true, true); break;
         case "current": r = s .current (); break;
         case "start": s .start = s .pos; break;
      }
      script .push ([op, arg, flag, r === undefined ? null : r, s .pos, s .start]);
   }
   streams .push ([line, tab, lineStart, script]);
}

const result = { cm: "5.65.21", generator: "oracle/cm5-util-dump.mjs", wordChar, extending, countColumn, findColumn, order, skipExtending, findFirst, streams };
await writeFile (OUT, JSON .stringify (result));
console .log (`util oracle -> ${OUT}`);
