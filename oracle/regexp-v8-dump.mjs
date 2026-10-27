/**
 * Regexp case oracle: record what V8 gives for JS regexps under the `i`, `u` and
 * `iu` flags, and what CodeMirror 5 `StringStream.match` gives on the same
 * regexps. crates/cm-core/tests/regexp_v8.rs and the `regexp.rs` unit tests
 * compare the Rust port against the output.
 *
 *   node oracle/regexp-v8-dump.mjs [out.json]     (default oracle/regexp-v8-baseline.json)
 *
 * Node only. Reads oracle/cm5-build/src/util/StringStream.js, never writes it.
 *
 * - `original`: the 30 cases of the PR-review triage (probes/regex/cases.json).
 * - `matrix`: every pattern under each of the flags "", "i", "u", "iu" against
 *   every input; one token per input: "-" no match, "E" SyntaxError, else
 *   "index:length". Inputs are hex UTF-16 code units (some are lone surrogates).
 * - `exec`: `exec` with `lastIndex = from`, the span of every group.
 * - `stream`: `StringStream.match(re, consume)` from a position.
 * - `orbits`: the case-equivalence classes of the regexp matcher, exhaustive:
 *   `u` holds every code point, `i` every code unit, that some `/[c]/iu` (or
 *   `/[c]/i`) matches besides itself. Candidates are the code points with a case
 *   mapping or folding (`\p{Cased}`, `\p{CWCM}`, `\p{CWCF}`); every other code
 *   point has no case variant.
 */
import { writeFile } from "node:fs/promises";
import { join, normalize } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = normalize (join (fileURLToPath (import.meta.url), ".."));
const { default: StringStream } = await import (pathToFileURL (join (HERE, "cm5-build", "src", "util", "StringStream.js")));

const OUT = process .argv [2] || join (HERE, "regexp-v8-baseline.json");

const FLAGS = ["", "i", "u", "iu"];

const ORIGINAL = [
   ["s", "i", "\u017f"], ["\u017f", "i", "s"], ["\u017f", "i", "S"], ["S", "i", "\u017f"], ["k", "i", "\u212a"], ["\u212a", "i", "k"],
   ["[a-z]", "iu", "\u212a"], ["[a-z]", "iu", "K"], ["[A-Z]", "iu", "a"], ["[A-Z]", "iu", "k"], ["[K]", "iu", "k"], ["[k]", "iu", "K"], ["[\\u212a]", "iu", "k"], ["[\\u212a]", "iu", "K"],
   ["[a-z]", "i", "\u212a"], ["[A-Z]", "i", "a"], ["[a-z]", "i", "Q"], ["[\u00e0-\u00ff]", "iu", "\u00c9"], ["[\u00c0-\u00de]", "iu", "\u00e9"], ["[\u00c0-\u00de]", "i", "\u00e9"],
   ["[a-z]", "iu", "\u017f"], ["[s]", "iu", "\u017f"], ["[\u017f]", "iu", "S"], ["[\u017f]", "i", "s"], ["[^a-z]", "iu", "K"], ["[^A-Z]", "iu", "a"], ["[\u0391-\u03a9]", "iu", "\u03c9"], ["[\u03b1-\u03c9]", "i", "\u03a9"], ["\\w", "iu", "\u017f"], ["[\\w]", "iu", "\u212a"],
];

// Literals, classes, ranges, negations and escapes around the case pairs whose
// `i` and `iu` canonicalizations differ: ſ/s, K/k (Kelvin), ς/σ, µ/μ, ı/İ, ß/ẞ,
// Cherokee (folds to upper case), ΐ/ΐ and ﬅ/ﬆ (fold without a case mapping),
// ǅ (title case), ϐ/β, Deseret (supplementary).
const PATTERNS = [
   "a", "A", "k", "K", "\\u212a", "s", "S", "\\u017f", "\\u03c3", "\\u00b5", "\\u0131", "\\u00df", "\\uab70", "\\u1fd3", "\\u01c5", "\\ud801\\udc00",
   "[a-z]", "[A-Z]", "[^a-z]", "[^A-Z]", "[K]", "[k]", "[\\u212a]", "[^\\u212a]", "[s]", "[S]", "[\\u017f]", "[^\\u017f]", "[a-jl-z]", "[t-z\\u017f]",
   "[\\u0391-\\u03a9]", "[\\u03b1-\\u03c9]", "[\\u03c2]", "[\\u03a3]", "[^\\u03c3]", "[\\u00b5]", "[\\u039c]", "[\\u00c0-\\u00de]", "[\\u00e0-\\u00ff]",
   "[i]", "[I]", "[\\u0130]", "[\\u0131]", "[\\u00df]", "[\\u1e9e]", "[\\uab70]", "[\\u13a0]", "[\\u13f8]", "[\\u1fd3]", "[\\u0390]", "[\\ufb05]",
   "[\\u01c4]", "[\\u01c5]", "[\\u01c6]", "[\\u03d0]", "[\\u03b2]", "[\\ud801\\udc00]", "[^\\ud801\\udc00]", "[\\ud801\\udc00-\\ud801\\udc27]", "[\\udc00]",
   "\\w", "\\W", "[\\w]", "[\\W]", "[^\\w]", "[^\\W]", "[\\W\\d]", "[\\s]", "\\d", ".",
];

const INPUTS = [
   "a", "A", "k", "K", "\u212a", "s", "S", "\u017f", "t", "\u03c9", "\u03a9", "\u03c2", "\u03c3", "\u03a3", "\u00b5", "\u03bc", "\u039c",
   "\u00e9", "\u00c9", "\u00ff", "\u0178", "i", "I", "\u0131", "\u0130", "\u00df", "\u1e9e", "\uab70", "\u13a0", "\u13f8", "\u13f0",
   "\u1fd3", "\u0390", "\ufb05", "\ufb06", "\u01c4", "\u01c5", "\u01c6", "\u03d0", "\u03b2", "\u0392",
   "\u{10400}", "\u{10428}", "\u{10427}", "\ud801", "\udc00", "1", "_", " ", "\u2028",
];

function tok (pattern, flags, input)
{
   let re;
   try { re = new RegExp (pattern, flags); } catch { return "E"; }
   const m = re .exec (input);
   return m ? `${m .index}:${m [0] .length}` : "-";
}

const original = ORIGINAL .map (([p, f, s]) => [p, f, s, tok (p, f, s)]);
const rows     = [];
for (const p of PATTERNS)
   for (const f of FLAGS)
      rows .push ([p, f, INPUTS .map (s => tok (p, f, s)) .join (" ")]);

// exec: [pattern, flags, input, from] -> spans of every group (null = undefined),
// or null for no match, or "E".
const EXEC = [
   ["([A-Z]+)(\\d)", "iu", "--abc1--", 0],
   ["([A-Z]+)(\\d)", "i", "--abc1--", 0],
   ["([A-Z]+)(\\d)", "u", "--abc1--", 0],
   ["(?<w>[K]+)\\k<w>", "iu", "xkK\u212akkK", 0],
   ["([K])\\1", "iu", "k\u212a", 0],
   ["([s])\\1", "iu", "s\u017f", 0],
   ["([s])\\1", "i", "s\u017f", 0],
   ["(\\ud801\\udc00)\\1", "iu", "\u{10400}\u{10428}", 0],
   ["(\\ud801\\udc00)\\1", "i", "\u{10400}\u{10428}", 0],
   ["[^A-Z]+", "iu", "abc\u017fdef123", 0],
   ["[^A-Z]+", "i", "abc\u017fdef123", 0],
   ["[A-Z]+", "iu", "123abc\u017f\u212axyz!", 0],
   ["[A-Z]+", "i", "123abc\u017f\u212axyz!", 0],
   ["\\w+", "iu", "  \u017f\u212as1_ ", 0],
   ["\\w+", "i", "  \u017f\u212as1_ ", 0],
   ["\\W+", "iu", "ab\u017f-+\u212a", 0],
   ["\\W+", "u", "ab\u017f-+\u212a", 0],
   ["a\\b", "iu", "a\u017f a", 0],
   ["a\\b", "i", "a\u017f a", 0],
   ["\\B\u017f", "iu", "a\u017f", 0],
   ["(?<=[K])x", "iu", "kx", 0],
   ["(?<![S])x", "iu", "\u017fx tx", 0],
   ["[\\u0391-\\u03a9]+", "iu", "--\u03b1\u03b2\u03c2\u03c9\u03d0--", 0],
   ["[\\u0391-\\u03a9]+", "i", "--\u03b1\u03b2\u03c2\u03c9\u03d0--", 0],
   ["[\\ud801\\udc00-\\ud801\\udc27]+", "iu", "x\u{10428}\u{10400}\u{1044f}y", 0],
   ["[\\ud801\\udc00-\\ud801\\udc27]+", "u", "x\u{10428}\u{10400}\u{1044f}y", 0],
   ["[A-Z]", "giu", "12k\u212a", 1],
   ["[A-Z]", "giu", "12k\u212a", 3],
   ["[A-Z]", "yiu", "12k\u212a", 2],
   ["[A-Z]", "yiu", "12k\u212a", 1],
   ["(?:([a-z])|([\\u212a]))+", "iu", "K\u212ak", 0],
   ["[a-z]{2,}?(\\d)", "iu", "\u212aS\u017f9", 0],
   ["^[\\W]*$", "iu", "--\u017f--", 0],
   ["^[^\\w]*$", "iu", "--\u212a--", 0],
   ["[\\w-]+", "iu", "\u212a-\u017f", 0],
];

function spans (re, s, from)
{
   re .lastIndex = from;
   const m = re .exec (s);
   if (!m) return null;
   // Group spans from `d` (hasIndices).
   return m .indices .map (x => x === undefined ? null : [x [0], x [1]]);
}

const exec = EXEC .map (([p, f, s, from]) =>
{
   let re;
   try { re = new RegExp (p, f + "d"); } catch { return [p, f, s, from, "E"]; }
   return [p, f, s, from, spans (re, s, from)];
});

// stream: [line, pos, pattern, flags, consume] -> [match array (UTF-16 code
// units, null = undefined) or null, `match.index` (null = undefined), pos after].
const STREAM = [
   ["ABC def", 0, "[a-z]+", "iu", true],
   ["ABC def", 0, "[a-z]+", "iu", false],
   ["ABC def", 0, "[a-z]+", "u", true],
   ["ABC def", 3, "[a-z]+", "iu", true],
   ["ABC def", 4, "[a-z]+", "iu", true],
   ["\u212aelvin", 0, "[k]\\w*", "iu", true],
   ["\u212aelvin", 0, "[k]\\w*", "i", true],
   ["x\u017fs", 1, "([S])([S])", "iu", true],
   ["x\u017fs", 1, "([S])([S])", "i", true],
   ["\u03c9\u03a9 \u03c2\u03a3", 0, "[\\u0391-\\u03a9]+", "iu", true],
   ["\u03c9\u03a9 \u03c2\u03a3", 3, "[\\u0391-\\u03a9]+", "i", true],
   ["\u{10428}\u{10400}!", 0, "[\\ud801\\udc00]+", "iu", true],
   ["\u{10428}\u{10400}!", 0, "[\\ud801\\udc00]+", "i", true],
   ["ab\u017f", 0, "[^A-Z]", "iu", true],
   ["1ab\u017f", 0, "[^A-Z]", "iu", true],
   ["--k", 0, "[K]", "giu", true],
   ["k--k", 0, "([K])-", "giu", true],
   ["k--k", 0, "([K])(-)", "iu", true],
   ["", 0, "[K]?", "iu", true],
];

const units = s => [...Array (s .length) .keys ()] .map (k => s .charCodeAt (k));
const stream = STREAM .map (([line, pos, p, f, consume]) =>
{
   const st = new StringStream (line, 4);
   st .pos = pos;
   const m = st .match (new RegExp (p, f), consume);
   return [line, pos, p, f, consume, m ? [...m] .map (x => x === undefined ? null : units (x)) : null, m && m .index !== undefined ? m .index : null, st .pos];
});

// Exhaustive case-equivalence classes.
const CASED = /[\p{Cased}\p{CWCM}\p{CWCF}]/u;
const hex   = c => c .toString (16) .padStart (4, "0");

function orbits (limit, flags, chr, esc)
{
   const cand = [];
   for (let c = 0; c < limit; ++c)
      if ((c < 0xd800 || c > 0xdfff) && CASED .test (String .fromCodePoint (c)))
         cand .push (c);
   const seen = new Set (), out = [];
   for (const c of cand)
   {
      if (seen .has (c)) continue;
      const re = new RegExp (`^[${esc (c)}]$`, flags);
      const o  = cand .filter (d => re .test (chr (d)));
      for (const d of o) seen .add (d);
      if (o .length > 1) out .push (o .map (hex) .join (" "));
   }
   return { candidates: cand .length, classes: out };
}

const u = orbits (0x110000, "iu", c => String .fromCodePoint (c), c => `\\u{${c .toString (16)}}`);
const i = orbits (0x10000, "i", c => String .fromCharCode (c), c => `\\u${hex (c)}`);

const out = {
   generator: "oracle/regexp-v8-dump.mjs",
   runtime: { node: process .version, v8: process .versions .v8, icu: process .versions .icu, unicode: process .versions .unicode },
   flags: FLAGS,
   original,
   matrix: { inputs: INPUTS .map (s => [...Array (s .length) .keys ()] .map (k => hex (s .charCodeAt (k))) .join (" ")), rows },
   exec,
   stream,
   orbits: { u, i },
};

await writeFile (OUT, JSON .stringify (out, null, 1) + "\n");
console .log (`${OUT}: ${original .length} original, ${rows .length} matrix rows x ${INPUTS .length}, ${exec .length} exec, ${stream .length} stream, ${u .classes .length} u / ${i .classes .length} i orbits`);
