/**
 * C2 differential oracle: seeded random sequences of CodeMirror.Doc operations run on
 * the real CodeMirror 5 build in headless chromium. Records the concrete operations
 * and JSON-serialized results; crates/cm-core/tests/fuzz_oracle.rs replays the
 * operations in Rust and must produce byte-identical JSON.
 *
 *   node oracle/cm5-doc-ops.mjs [--seeds N] [--first S] [--ops K] [--out file.json]
 *                               [--stacks] [--debug] [--hook file.js]
 *
 * --stacks keeps JS error stacks per sequence, --debug adds per-op marker/history
 * internals, --hook loads a script defining window.__hook (k, op, docs, log).
 * 30% of sequences attach doc 0 to a real CodeMirror editor ("editor mode": every
 * call runs in an operation, plus "batch" ops grouped with cm.operation); the Rust
 * side uses Doc::attach_editor_semantics for those.
 *
 * Defaults: 600 sequences (seeds 1..600) of 60 operations, written to
 * oracle/cm5-doc-ops-baseline.json.
 *
 * `Date` is replaced by a fake clock (advanced only by the "tick" op) so history
 * event merging, which depends on wall-clock time, is deterministic.
 */
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { extname, join, normalize } from "node:path";
import { safeResolve } from "./server-utils.mjs";
import { fileURLToPath } from "node:url";

const HERE = normalize (join (fileURLToPath (import.meta.url), ".."));
const ROOT = join (HERE, "cm5-build");
const { chromium } = createRequire (process .env .CM5_PLAYWRIGHT_FROM || join (HERE, "../../../x_ite-tests/")) ("playwright");

const args = process .argv .slice (2);
const opt = (name, def) => { const i = args .indexOf (name); return i >= 0 ? args [i + 1] : def; };
const SEEDS = Number (opt ("--seeds", 600));
const DEBUG = args .includes ("--debug");
const STACKS = args .includes ("--stacks");
const FIRST = Number (opt ("--first", 1));
const NOPS  = Number (opt ("--ops", 60));
const OUT   = opt ("--out", join (HERE, "cm5-doc-ops-baseline.json"));

const HARNESS = `<!doctype html><meta charset="utf-8"><script src="/lib/codemirror.js"></script>`;

const server = createServer (async (req, res) =>
{
   const url = new URL (req .url, "http://x");
   if (url .pathname === "/__harness.html")
   {
      res .writeHead (200, { "Content-Type": "text/html", "Cache-Control": "no-store" });
      return res .end (HARNESS);
   }
   const s = safeResolve (url .pathname, ROOT);
   if (s .status !== 200) { res .writeHead (s .status); return res .end (); }
   const path = s .path;
   try
   {
      const body = await readFile (path);
      res .writeHead (200, { "Content-Type": extname (path) === ".js" ? "text/javascript" : "application/octet-stream", "Cache-Control": "no-store" });
      res .end (body);
   }
   catch { res .writeHead (404); res .end (); }
});

await new Promise (r => server .listen (0, "127.0.0.1", r));

const browser = await chromium .launch ({ headless: true });
const page    = await browser .newPage ();

// Fake clock, installed before codemirror.js loads.
await page .addInitScript (() =>
{
   const RealDate = Date;
   window .__now = 1e12;
   class FakeDate extends RealDate
   {
      constructor (...a) { if (a .length === 0) super (window .__now); else super (...a); }
      static now () { return window .__now; }
   }
   window .Date = FakeDate;
});

await page .goto (`http://127.0.0.1:${server .address () .port}/__harness.html`);
await page .waitForFunction (() => window .CodeMirror && CodeMirror .Doc);

// ---------------------------------------------------------------------------------
// Everything below runs in the page.

function runSequences (seeds, nops, debug)
{
   const Pos = CodeMirror .Pos;

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

   function runSeq (seed)
   {
      const rnd  = mulberry32 (seed);
      const ri   = n => Math .floor (rnd () * n);
      const pick = arr => arr [ri (arr .length)];
      const chance = p => rnd () < p;

      window .__now = 1e12;

      const PIECES = ["a", "b", "c", "d", " ", "xy", "\n", "\n", "é", "\t", "😀", "\r\n", "\r", "ab\ncd", "", "́"];
      const randText = max => { let s = ""; for (let i = ri (max + 1); i > 0; --i) s += pick (PIECES); return s; };

      const init = {
         text: randText (8),
         firstLine: pick ([0, 0, 0, 0, 2]),
         lineSep: pick ([null, null, null, null, "\n", "\r\n"]),
         undoDepth: pick ([null, null, null, null, 1, 2, 3, 5]),
         editor: chance (0.3),
      };

      const docs = [new CodeMirror .Doc (init .text, null, init .firstLine, init .lineSep)];
      // Editor mode: attach doc 0 to a real (headless) CodeMirror instance, so every
      // API call runs in an operation, with the editor's option defaults.
      const cm = init .editor ? CodeMirror (document .body, { value: docs [0], lineSeparator: init .lineSep }) : null;
      if (cm)
      {
         // View-only: cursor/selection drawing at the end of every operation measures
         // the selection and throws on the corrupted states some CM5 bugs produce,
         // replacing the model's own result. The Rust port has no view, so disable it.
         cm .display .input .prepareSelection = () => ({ cursors: document .createDocumentFragment (), selection: document .createDocumentFragment () });
         cm .display .input .showSelection = () => {};
      }
      if (init .undoDepth != null) docs [0] .history .undoDepth = init .undoDepth;

      const marks = [];
      const labels = new Map ();
      let nextLabel = 0;
      const label = m => { if (!labels .has (m)) labels .set (m, nextLabel ++); return labels .get (m); };

      // -- serialization (mirrored exactly by the Rust replay)
      const P   = p => p ? [p .line, p .ch, p .sticky == null ? null : p .sticky] : null;
      const SEL = d => [d .listSelections () .map (r => [P (r .anchor), P (r .head)]), d .sel .primIndex];
      const FIND = m => { const f = m .find (); if (!f) return null; if (f .from === undefined && f .line !== undefined) return P (f); return [P (f .from), f .to ? P (f .to) : null]; };
      const HIST = d =>
      {
         const h = d .getHistory ();
         const ev = e => e .ranges
            ? ["s", e .ranges .map (r => [P (r .anchor), P (r .head)]), e .primIndex]
            : ["c", e .changes .map (c => [P (c .from), P (c .to), c .text .join ("\n")])];
         return [h .done .map (ev), h .undone .map (ev)];
      };
      const SNAP = d => { const hs = d .historySize (); return [d .getValue (), SEL (d), hs .undo, hs .redo]; };

      // -- argument generation
      const genPos = d =>
      {
         const first = d .firstLine (), size = d .lineCount ();
         const line = first + ri (size + 2) - (chance (0.1) ? 2 : 0) - (chance (0.15) ? 1 : 0);
         let ch;
         if (chance (0.1)) ch = null;
         else
         {
            const l = d .getLine (line);
            ch = ri ((l == null ? 4 : l .length) + 3) - 1;
         }
         const sticky = chance (0.08) ? pick (["before", "after"]) : null;
         return [line, ch, sticky];
      };
      const mk = a => a == null ? undefined : (a [1] == null ? Pos (a [0]) : Pos (a [0], a [1], a [2]));
      const ORIGINS = [null, null, "+input", "+input", "+delete", "*compose", "paste", "+move", "foo"];
      const genSelOpts = () => chance (0.5) ? null : {
         origin: pick (ORIGINS),
         bias: pick ([null, null, -1, 1]),
         clearRedo: pick ([null, null, false, true]),
      };
      const selOpts = o => o == null ? undefined : (() => {
         const r = {};
         if (o .origin != null) r .origin = o .origin;
         if (o .bias != null) r .bias = o .bias;
         if (o .clearRedo != null) r .clearRedo = o .clearRedo;
         return r;
      }) ();
      const genMarkOpts = shared => {
         const o = {};
         if (chance (0.3)) o .inclusiveLeft = true;
         if (chance (0.3)) o .inclusiveRight = true;
         if (chance (0.25)) o .atomic = true;
         if (chance (0.12)) o .readOnly = true;
         if (chance (0.2)) o .clearWhenEmpty = false;
         if (chance (0.15)) o .addToHistory = true;
         // Collapsed markers drive the editor's height estimation and visual-line
         // bookkeeping (view code), so they are only fuzzed on bare documents.
         if (!cm && chance (0.08)) o .collapsed = true;
         if (!shared && chance (0.1)) o .clearOnEnter = true;
         if (chance (0.15)) o .selectLeft = chance (0.5);
         if (chance (0.15)) o .selectRight = chance (0.5);
         if (shared) o .shared = true;
         return o;
      };

      const WEIGHTS = [
         ["replaceRange", 18], ["replaceSelection", 8], ["setSelection", 8], ["setCursor", 4], ["setSelections", 4],
         ["addSelection", 3], ["extendSelection", 3], ["extendSelections", 1], ["undo", 8], ["redo", 5],
         ["undoSelection", 3], ["redoSelection", 2], ["setValue", 2], ["markText", 7], ["setBookmark", 3],
         ["clearMarker", 3], ["tick", 8], ["clearHistory", 1], ["markClean", 1], ["changeGeneration", 1],
         ["isClean", 1], ["linkedDoc", 2], ["unlinkDoc", 1], ["copyDoc", 1], ["historyRoundtrip", 1],
         ["setExtending", 1], ["getRange", 3], ["posFromIndex", 2], ["indexFromPos", 2], ["findMarksAt", 2],
         ["findMarks", 1], ["getCursor", 2], ["getSelections", 1], ["getLine", 1], ["clipPos", 1],
         ["markerFind", 2], ["getAllMarks", 1], ["getHistory", 1], ["selectAll", 1], ["batch", 6],
      ];
      const BATCHABLE = new Set (["replaceRange", "replaceSelection", "setSelection", "setCursor", "setSelections",
         "addSelection", "extendSelection", "undo", "redo", "undoSelection", "redoSelection", "setValue", "markText",
         "setBookmark", "clearMarker", "tick", "getRange", "getCursor", "selectAll"]);
      const TOTAL = WEIGHTS .reduce ((a, w) => a + w [1], 0);
      const pickOp = () => { let r = ri (TOTAL); for (const [n, w] of WEIGHTS) { if (r < w) return n; r -= w; } };

      function genOp (inBatch)
      {
         for (;;)
         {
            const kind = pickOp ();
            if (inBatch && !BATCHABLE .has (kind)) continue;
            const di = inBatch ? 0 : ri (docs .length), d = docs [di];
            switch (kind)
            {
               case "batch":
               {
                  if (!cm || inBatch) continue;
                  const subs = [];
                  for (let n = 2 + ri (3); n > 0; --n) subs .push (genOp (true));
                  return { op: kind, subs };
               }
               case "replaceRange": return { op: kind, d: di, text: randText (4), from: genPos (d), to: chance (0.3) ? null : genPos (d), origin: pick (ORIGINS) };
               case "replaceSelection": return { op: kind, d: di, text: randText (3), collapse: pick ([null, null, "around", "start", "end"]), origin: pick (ORIGINS) };
               case "setSelection": return { op: kind, d: di, anchor: genPos (d), head: chance (0.3) ? null : genPos (d), opts: genSelOpts () };
               case "setCursor": return { op: kind, d: di, pos: genPos (d), opts: genSelOpts () };
               case "setSelections":
               {
                  const n = 1 + ri (3), ranges = [];
                  for (let i = 0; i < n; ++i) ranges .push ([genPos (d), chance (0.3) ? null : genPos (d)]);
                  return { op: kind, d: di, ranges, primary: chance (0.5) ? null : ri (n), opts: genSelOpts () };
               }
               case "addSelection": return { op: kind, d: di, anchor: genPos (d), head: chance (0.4) ? null : genPos (d), opts: genSelOpts () };
               case "extendSelection": return { op: kind, d: di, head: genPos (d), other: chance (0.5) ? null : genPos (d), opts: genSelOpts () };
               case "extendSelections": return { op: kind, d: di, heads: d .listSelections () .map (() => genPos (d)) };
               case "setValue":
               {
                  const text = randText (6);
                  // CM5 throws mid-change for setValue on a doc starting past line 0 when
                  // the new text has fewer lines than `first` (see updateDoc); skip that.
                  if (d .firstLine () > 0 && d .splitLines (text) .length < d .firstLine ()) continue;
                  return { op: kind, d: di, text };
               }
               case "markText":
               {
                  const shared = docs .length > 1 && chance (0.3);
                  return { op: kind, d: di, from: genPos (d), to: genPos (d), opts: genMarkOpts (shared) };
               }
               case "setBookmark": return { op: kind, d: di, pos: genPos (d), insertLeft: chance (0.5), shared: docs .length > 1 && chance (0.2) };
               case "clearMarker": if (!marks .length) continue; return { op: kind, m: ri (marks .length) };
               case "markerFind": if (!marks .length) continue; return { op: kind, m: ri (marks .length) };
               case "tick": return { op: kind, ms: pick ([10, 100, 400, 600, 1300, 3000]) };
               case "changeGeneration": return { op: kind, d: di, force: chance (0.5) };
               case "isClean": return { op: kind, d: di };
               case "linkedDoc":
               {
                  if (docs .length >= 4) continue;
                  const first = d .firstLine (), size = d .lineCount ();
                  let from = chance (0.5) ? null : first + ri (size);
                  let to = chance (0.5) ? null : first + 1 + ri (size + 1);
                  const f = from == null ? first : Math .max (first, from), t = to == null ? first + size : Math .min (first + size, to);
                  if (f >= t) continue; // CM5 builds an "undefined" document then
                  return { op: kind, d: di, from, to, sharedHist: chance (0.5) };
               }
               case "unlinkDoc": if (docs .length < 2) continue; return { op: kind, d: di, other: ri (docs .length) };
               case "copyDoc": if (docs .length >= 4) continue; return { op: kind, d: di, copyHistory: chance (0.5) };
               case "setExtending": return { op: kind, d: di, val: chance (0.5) };
               case "getRange": return { op: kind, d: di, from: genPos (d), to: genPos (d), sep: pick ([null, null, "", "|"]) };
               case "posFromIndex": return { op: kind, d: di, index: ri (d .getValue () .length + 6) - 2 };
               case "indexFromPos": return { op: kind, d: di, pos: genPos (d) };
               case "findMarksAt": return { op: kind, d: di, pos: genPos (d) };
               case "findMarks": return { op: kind, d: di, from: genPos (d), to: genPos (d) };
               case "getCursor": return { op: kind, d: di, which: pick ([null, "head", "anchor", "start", "end", "from", "to"]) };
               case "getLine": return { op: kind, d: di, line: d .firstLine () + ri (d .lineCount () + 2) - 1 };
               case "clipPos": return { op: kind, d: di, pos: genPos (d) };
               default: return { op: kind, d: di };
            }
         }
      }

      function apply (o)
      {
         const d = docs [o .d];
         switch (o .op)
         {
            case "replaceRange": d .replaceRange (o .text, mk (o .from), mk (o .to), o .origin == null ? undefined : o .origin); return SNAP (d);
            case "replaceSelection": d .replaceSelection (o .text, o .collapse == null ? undefined : o .collapse, o .origin == null ? undefined : o .origin); return SNAP (d);
            case "setSelection": d .setSelection (mk (o .anchor), mk (o .head), selOpts (o .opts)); return SNAP (d);
            case "setCursor": d .setCursor (mk (o .pos), null, selOpts (o .opts)); return SNAP (d);
            case "setSelections": d .setSelections (o .ranges .map (r => ({ anchor: mk (r [0]), head: mk (r [1]) })), o .primary == null ? undefined : o .primary, selOpts (o .opts)); return SNAP (d);
            case "addSelection": d .addSelection (mk (o .anchor), mk (o .head), selOpts (o .opts)); return SNAP (d);
            case "extendSelection": d .extendSelection (mk (o .head), mk (o .other), selOpts (o .opts)); return SNAP (d);
            case "extendSelections": d .extendSelections (o .heads .map (mk)); return SNAP (d);
            case "undo": d .undo (); return SNAP (d);
            case "redo": d .redo (); return SNAP (d);
            case "undoSelection": d .undoSelection (); return SNAP (d);
            case "redoSelection": d .redoSelection (); return SNAP (d);
            case "setValue": d .setValue (o .text); return SNAP (d);
            case "selectAll": d .setSelection (Pos (d .firstLine (), 0), Pos (d .lastLine ()), { scroll: false }); return SNAP (d);
            case "markText":
            {
               const m = d .markText (mk (o .from), mk (o .to), o .opts);
               marks .push (m);
               return [label (m), FIND (m), SNAP (d)];
            }
            case "setBookmark":
            {
               const m = d .setBookmark (mk (o .pos), { insertLeft: o .insertLeft, shared: o .shared });
               marks .push (m);
               return [label (m), FIND (m), SNAP (d)];
            }
            case "clearMarker": { const m = marks [o .m]; m .clear (); return [label (m), SNAP (docs [0])]; }
            case "markerFind": { const m = marks [o .m]; return [label (m), FIND (m)]; }
            case "tick": window .__now += o .ms; return null;
            case "clearHistory": d .clearHistory (); return SNAP (d);
            case "markClean": d .markClean (); return d .isClean ();
            case "changeGeneration": return d .changeGeneration (o .force);
            case "isClean": return d .isClean ();
            case "linkedDoc":
            {
               const opts = { sharedHist: o .sharedHist };
               if (o .from != null) opts .from = o .from;
               if (o .to != null) opts .to = o .to;
               const n = d .linkedDoc (opts);
               docs .push (n);
               return SNAP (n);
            }
            case "unlinkDoc": d .unlinkDoc (docs [o .other]); return [SNAP (d), SNAP (docs [o .other])];
            case "copyDoc": { const n = d .copy (o .copyHistory); docs .push (n); return [SNAP (n), HIST (n)]; }
            case "historyRoundtrip": d .setHistory (JSON .parse (JSON .stringify (d .getHistory ()))); return HIST (d);
            case "setExtending": d .setExtending (o .val); return d .getExtending ();
            case "getRange": return d .getRange (mk (o .from), mk (o .to), o .sep == null ? undefined : o .sep);
            case "posFromIndex": return P (d .posFromIndex (o .index));
            case "indexFromPos": return d .indexFromPos (mk (o .pos));
            case "findMarksAt": return d .findMarksAt (mk (o .pos)) .map (label);
            case "findMarks": return d .findMarks (mk (o .from), mk (o .to)) .map (label);
            case "getCursor": return P (d .getCursor (o .which == null ? undefined : o .which));
            case "getSelections": return [d .getSelections (), d .getSelection (), d .somethingSelected ()];
            case "getLine": { const l = d .getLine (o .line); return l === undefined ? null : l; }
            case "clipPos": return P (d .clipPos (mk (o .pos)));
            case "getAllMarks": return d .getAllMarks () .map (label);
            case "getHistory": return HIST (d);
            case "batch": return cm .operation (() => o .subs .map (apply));
         }
         throw new Error ("unknown op " + o .op);
      }

      const ops = [], out = [], stacks = [];
      for (let k = 0; k < nops; ++k)
      {
         const o = genOp ();
         ops .push (o);
         if (window .__hook) window .__hook (k, o, docs, stacks);
         let r;
         try { r = apply (o); }
         catch (e) { r = { err: e instanceof TypeError ? "TypeError" : String (e .message) }; stacks .push ([k, String (e .stack) .split ("\n") .slice (0, 8) .join (" | ")]); }
         out .push (r);
         if (debug)
         {
            // Marker bookkeeping per doc: spans per line and each marker's line list.
            const dd = docs .map (d => {
               const lines = [];
               d .iter (d .firstLine (), d .firstLine () + d .lineCount (), l => { lines .push ((l .markedSpans || []) .map (sp => [label (sp .marker), sp .from, sp .to])); });
               return lines;
            });
            const ml = [];
            for (const [m, lab] of labels) if (m .lines) ml .push ([lab, m .lines .map (l => { const n = l .lineNo (); return n == null ? null : n; })]);
            const sz = docs .map (d => { let n = 0; d .iterN (0, 1e9, () => { ++n; }); return [d .first, d .size, n]; });
            const hi = docs .map (d => { const h = d .history; return [h .lastOrigin == null ? null : h .lastOrigin, h .lastSelOrigin == null ? null : h .lastSelOrigin, h .generation, h .maxGeneration, h .done .length, h .undone .length, h .undoDepth === Infinity ? -1 : h .undoDepth]; });
            stacks .push (["dbg", k, JSON .stringify ([dd, ml, sz, hi, docs .map (HIST)])]);
         }
      }
      // Final dump of every document and marker.
      out .push (docs .map (d => [d .firstLine (), d .lineCount (), SNAP (d), HIST (d), d .isClean (), d .getAllMarks () .map (m => [label (m), FIND (m)])]));
      out .push (marks .map (m => [label (m), FIND (m)]));
      if (cm) cm .getWrapperElement () .remove ();
      const r = { seed, init, ops, out: JSON .stringify (out) };
      if (debug || window .__hook || window .__keepStacks) r .errors = stacks;   // stacks carry the server port: debug only
      return r;
   }

   return seeds .map (runSeq);
}

// ---------------------------------------------------------------------------------

await page .addScriptTag ({ content: runSequences .toString () });
const HOOK = opt ("--hook", null);   // debugging: a script defining window.__hook (k, op, docs, log)
if (HOOK) await page .addScriptTag ({ content: await readFile (HOOK, "utf8") });
if (STACKS) await page .evaluate (() => window .__keepStacks = true);

const results = [];
const CHUNK = 50;
for (let s = FIRST; s < FIRST + SEEDS; s += CHUNK)
{
   const seeds = [];
   for (let k = s; k < Math .min (FIRST + SEEDS, s + CHUNK); ++k) seeds .push (k);
   results .push (... await page .evaluate (([seeds, nops, debug]) => runSequences (seeds, nops, debug), [seeds, NOPS, DEBUG]));
}

await browser .close ();
server .close ();

let errors = 0;
for (const r of results) errors += (r .out .match (/"err":/g) || []) .length;

await writeFile (OUT, JSON .stringify ({ cm: "5.65.21", generator: "oracle/cm5-doc-ops.mjs", ops: NOPS, sequences: results }));
console .log (`${results .length} sequences x ${NOPS} ops -> ${OUT} (${errors} ops threw in JS)`);
