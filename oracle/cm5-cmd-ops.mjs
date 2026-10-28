/**
 * C5 differential oracle: seeded random sequences of editor *commands* (the
 * `CodeMirror.commands` table), shifted motion, typing, copy/cut/paste and selection
 * changes, run on a real CodeMirror 5 editor in headless chromium. Records the
 * concrete steps and the editor state after each one; crates/cm-view/tests/
 * cmd_fuzz_oracle.rs replays them on cm_view::Editor and must match exactly.
 *
 *   node oracle/cm5-cmd-ops.mjs [--seeds N] [--first S] [--steps K] [--out file.json]
 *
 * Determinism measures:
 * - Measurement: DejaVu Sans Mono 32px, line-height 38px. Its advance is
 *   19.265625px (a multiple of 1/64), so Chromium's text positions are exact
 *   multiples and equal the Rust box model (`Metrics::oracle()`). The page aborts if
 *   charWidth/textHeight/emoji width differ. Start documents from corpus-items are
 *   restricted to ASCII (+ tab/CR/LF) and <= 3000 lines; gzipped files are skipped.
 * - Time: `Date` is a fake clock advanced only by "tick" steps (history merging and
 *   `pasteIncoming`/`cutIncoming` depend on it).
 * - Mode state: before EVERY step, `cm.getStateAfter(cm.lastLine(), true)` brings
 *   every line's saved mode state up to date. This models a caught-up background
 *   highlight worker; without it, which `stateAfter` values exist depends on what
 *   was rendered and on worker timers. The Rust replay calls
 *   `Editor::catch_up_states(None, true)` at the same points.
 * - Input: `applyTextInput` (and lastCopied/copyableRanges) is not exported by the
 *   bundle, so the page fetches lib/codemirror.js and evaluates an in-memory copy
 *   with a few `window.__*` accessors added; the files on disk are untouched.
 *
 * Document values (and copied text) are stored in full up to 4000 UTF-16 units
 * without lone surrogates, otherwise as
 * "#<len>:<fnv1a32><fnv1a32'>" (see `hashText`).
 *
 * Defaults: 600 sequences (seeds 1..600) of 36 steps -> oracle/cm5-cmd-ops-baseline.json.
 */
import { createServer } from "node:http";
import { readFile, writeFile, readdir } from "node:fs/promises";
import { createRequire } from "node:module";
import { extname, join, normalize, relative, isAbsolute } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = normalize (join (fileURLToPath (import.meta.url), ".."));
const ROOT = join (HERE, "cm5-build");
const CORPUS = join (HERE, "corpus-items");
const { chromium } = createRequire (process .env .CM5_PLAYWRIGHT_FROM || join (HERE, "../../../x_ite-tests/")) ("playwright");

const args = process .argv .slice (2);
const opt = (name, def) => { const i = args .indexOf (name); return i >= 0 ? args [i + 1] : def; };
const SEEDS = Number (opt ("--seeds", 600));
const FIRST = Number (opt ("--first", 1));
const NSTEPS = Number (opt ("--steps", 36));
const OUT   = opt ("--out", join (HERE, "cm5-cmd-ops-baseline.json"));

// ---------------------------------------------------------------------------------
// Corpus selection (read-only).

async function walk (dir)
{
   const out = [];
   for (const e of (await readdir (dir, { withFileTypes: true })) .sort ((a, b) => a .name < b .name ? -1 : a .name > b .name ? 1 : 0))
   {
      const p = join (dir, e .name);
      if (e .isDirectory ()) out .push (... await walk (p));
      else if (/\.(wrl|js|json)$/ .test (e .name)) out .push (p);
   }
   return out;
}

const corpus = [];
for (const p of await walk (CORPUS))
{
   const b = await readFile (p);
   if (b [0] === 0x1f && b [1] === 0x8b) continue;
   let ok = true, lines = 0;
   for (const x of b) { if (x === 10) ++lines; if (!(x === 9 || x === 10 || x === 13 || (x >= 0x20 && x <= 0x7e))) { ok = false; break; } }
   if (!ok || lines > 3000) continue;
   corpus .push ({ path: relative (HERE, p), text: b .toString ("latin1") });
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

const modeForPath = p => p .endsWith (".wrl") ? "model/vrml" : p .endsWith (".json") ? "application/json" : "javascript";

const specs = [];
for (let seed = FIRST; seed < FIRST + SEEDS; ++seed)
{
   const r = mulberry32 (seed * 7919 + 13);
   if (r () < 0.65)
   {
      const f = corpus [Math .floor (r () * corpus .length)];
      specs .push ({ seed, path: f .path, mode: modeForPath (f .path), text: f .text });
   }
   else
   {
      const modes = [null, "javascript", "model/vrml", "model/vrml", "text/html", "css", "xml", "application/json"];
      specs .push ({ seed, path: null, mode: modes [Math .floor (r () * modes .length)], text: null });
   }
}

// ---------------------------------------------------------------------------------
// Page

const HARNESS = `<!doctype html><meta charset="utf-8"><link rel="stylesheet" href="/lib/codemirror.css">
<style>body { margin: 0 } .CodeMirror { font-family: "DejaVu Sans Mono"; font-size: 32px; line-height: 38px; width: 900px; height: 500px }</style>`;

const server = createServer (async (req, res) =>
{
   const url = new URL (req .url, "http://x");
   if (url .pathname === "/__harness.html")
   {
      res .writeHead (200, { "Content-Type": "text/html", "Cache-Control": "no-store" });
      return res .end (HARNESS);
   }
   let s;
   if (url .pathname === "/__vrml.js") {
      // This is served from HERE, but safeResolve expects urlPath and rootDir.
      // So urlPath is just "cm5-vrml-mode.js" and rootDir is HERE.
      s = safeResolve ("cm5-vrml-mode.js", HERE);
   } else {
      // The original code resolved normal requests against ROOT but verified against HERE.
      // Wait, in cm5-cmd-ops, normal files are in ROOT (cm5-build).
      // But they allowed them to escape ROOT up to HERE.
      // If we use safeResolve against ROOT, does it break anything? The original issue said:
      // "say in the PR body whether switching to the ROOT-based safeResolve narrows or widens what that server can serve, and keep /__vrml.js served from HERE."
      // The reviewer says: "Use safeResolve for normal requests (against the root those files are actually served from; keep /__vrml.js as the one explicit asset, resolved through safeResolve too)."
      s = safeResolve (url .pathname, ROOT);
   }
   if (s .status !== 200) { res .writeHead (s .status); return res .end (); }
   const base = s .path;
   try
   {
      const body = await readFile (base);
      const type = { ".js": "text/javascript", ".css": "text/css" } [extname (base)] || "application/octet-stream";
      res .writeHead (200, { "Content-Type": type, "Cache-Control": "no-store" });
      res .end (body);
   }
   catch { res .writeHead (404); res .end (); }
});

await new Promise (r => server .listen (0, "127.0.0.1", r));

const browser = await chromium .launch ({ headless: true });
const page    = await browser .newPage ({ viewport: { width: 1200, height: 900 } });

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

// codemirror.js with accessors for module-internal input functions (in memory only).
let cmSource = await readFile (join (ROOT, "lib/codemirror.js"), "utf8");
const HOOK = "  function setLastCopied(newLastCopied) {";
if (!cmSource .includes (HOOK)) throw new Error ("patch point not found");
cmSource = cmSource .replace (HOOK,
   "  window.__applyTextInput = function () { return applyTextInput.apply(null, arguments); };\n" +
   "  window.__setLastCopied = function (v) { setLastCopied(v); };\n" +
   "  window.__getLastCopied = function () { return lastCopied; };\n" +
   "  window.__copyableRanges = function (cm) { return copyableRanges(cm); };\n" + HOOK);
await page .addScriptTag ({ content: cmSource });
for (const m of ["toml/toml.js", "xml/xml.js", "javascript/javascript.js", "css/css.js", "htmlmixed/htmlmixed.js"])
   await page .addScriptTag ({ url: "/mode/" + m });
await page .addScriptTag ({ url: "/__vrml.js" });
await page .waitForFunction (() => window .CodeMirror && window .__applyTextInput && CodeMirror .modes .vrml && CodeMirror .modes .htmlmixed);

// ---------------------------------------------------------------------------------
// Everything below runs in the page.

function runSequences (specs, nsteps)
{
   const Pos = CodeMirror .Pos;

   // Metrics check.
   {
      const cm = CodeMirror (document .body, { value: "x\u{1F600}y" });
      const w = cm .cursorCoords (Pos (0, 3), "div") .left - cm .cursorCoords (Pos (0, 1), "div") .left;
      const bad = cm .defaultCharWidth () !== 19.265625 || cm .defaultTextHeight () !== 38 || w !== 40;
      cm .getWrapperElement () .remove ();
      if (bad) throw new Error ("metrics differ: " + cm .defaultCharWidth () + " " + cm .defaultTextHeight () + " " + w);
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

   function hashText (s)
   {
      if (s .length <= 4000 && !/[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/ .test (s)) return s;
      let h1 = 0x811c9dc5, h2 = 0x01000193 ^ 0x5bd1e995;
      for (let i = 0; i < s .length; ++i)
      {
         const c = s .charCodeAt (i);
         h1 = Math .imul (h1 ^ c, 16777619) >>> 0;
         h2 = Math .imul (h2 ^ c, 0x01000193 + 2) >>> 0;
      }
      return "#" + s .length + ":" + h1 .toString (16) .padStart (8, "0") + h2 .toString (16) .padStart (8, "0");
   }

   const COMMANDS = [
      ["goCharLeft", 6], ["goCharRight", 6], ["goColumnLeft", 1], ["goColumnRight", 1],
      ["goWordLeft", 3], ["goWordRight", 3], ["goGroupLeft", 3], ["goGroupRight", 3],
      ["goLineStart", 2], ["goLineStartSmart", 3], ["goLineEnd", 3], ["goLineLeft", 1], ["goLineRight", 1], ["goLineLeftSmart", 1],
      ["goLineUp", 6], ["goLineDown", 6], ["goPageUp", 2], ["goPageDown", 2], ["goDocStart", 1], ["goDocEnd", 1],
      ["delCharBefore", 4], ["delCharAfter", 3], ["delWordBefore", 1], ["delWordAfter", 1], ["delGroupBefore", 2], ["delGroupAfter", 2],
      ["killLine", 2], ["deleteLine", 2], ["delLineLeft", 1], ["delWrappedLineLeft", 1], ["delWrappedLineRight", 1],
      ["undo", 4], ["redo", 2], ["undoSelection", 2], ["redoSelection", 1],
      ["indentAuto", 3], ["indentMore", 2], ["indentLess", 2], ["insertTab", 1], ["insertSoftTab", 1], ["defaultTab", 2],
      ["transposeChars", 2], ["newlineAndIndent", 5], ["openLine", 1], ["selectAll", 1], ["singleSelection", 1], ["toggleOverwrite", 1],
   ];
   const STEPS = [
      ["cmd", 60], ["shift", 14], ["type", 22], ["select", 5], ["addSelection", 3], ["setSelections", 2],
      ["extending", 1], ["tick", 6], ["copy", 2], ["cut", 2], ["paste", 3],
   ];
   const total = arr => arr .reduce ((a, w) => a + w [1], 0);
   const TYPE_PIECES = ["a", "b", "x", "foo", " ", "  ", "\t", "\n", "{", "}", "[", "]", "(", ")", ";", ",", "<", ">", "/", "</", "*/",
      "é", "́", "\u{1F600}", "DEF ", "#", "\"", "x\ny", "Transform {", "children [", "]\n}", "if (a) {", "\n}", "</div>"];
   const DOC_PIECES = ["a", "b", "foo", "bar", " ", " ", "\t", "\n", "\n", "\n", "{", "}", "[", "]", "(", ")", ";", "é", "́", "\u{1F600}",
      "Shape {", "geometry Box { size 1 2 3 }", "# comment", "function f(x) {", "return x;", "<div>", "</div>", "\"str\"", "  ", "1.5", ","];

   function runSeq (spec)
   {
      const rnd  = mulberry32 (spec .seed);
      const ri   = n => Math .floor (rnd () * n);
      const pick = arr => arr [ri (arr .length)];
      const chance = p => rnd () < p;
      const wpick = arr => { let r = ri (total (arr)); for (const [n, w] of arr) { if (r < w) return n; r -= w; } };

      window .__now = 1e12;
      __setLastCopied (null);

      let text = spec .text;
      if (text == null)
      {
         text = "";
         for (let i = 2 + ri (60); i > 0; --i) text += pick (DOC_PIECES);
      }
      const cm = CodeMirror (document .body, { value: text, mode: spec .mode });
      const init = { path: spec .path, mode: spec .mode, text: spec .path ? null : text };

      const P = p => [p .line, p .ch, p .sticky == null ? null : p .sticky];
      const SNAP = () =>
      {
         const hs = cm .historySize ();
         return [hashText (cm .getValue ()), cm .listSelections () .map (r => [P (r .anchor), P (r .head)]), cm .doc .sel .primIndex, hs .undo, hs .redo, !!cm .state .overwrite];
      };
      const genPos = () =>
      {
         // Bias towards an existing cursor so work stays near the selection.
         let line;
         if (chance (0.6)) line = Math .max (cm .firstLine (), Math .min (cm .lastLine (), cm .getCursor () .line + ri (7) - 3));
         else line = cm .firstLine () + ri (cm .lineCount ());
         const len = cm .getLine (line) .length;
         return [line, ri (len + 1)];
      };
      const mk = a => Pos (a [0], a [1]);

      let clipboard = "";
      const genStep = () =>
      {
         const kind = wpick (STEPS);
         switch (kind)
         {
            case "cmd": return { op: "cmd", name: wpick (COMMANDS) };
            case "shift": return { op: "shift", name: pick (["goCharLeft", "goCharRight", "goWordLeft", "goWordRight", "goGroupLeft", "goGroupRight",
               "goLineUp", "goLineDown", "goPageUp", "goPageDown", "goLineStartSmart", "goLineEnd", "goDocStart", "goDocEnd", "goLineRight", "goLineLeft"]) };
            case "type": { let t = ""; for (let i = 1 + (chance (0.3) ? ri (3) : 0); i > 0; --i) t += pick (TYPE_PIECES); return { op: "type", text: t }; }
            case "select": return { op: "select", anchor: genPos (), head: chance (0.5) ? null : genPos () };
            case "addSelection": return { op: "addSelection", anchor: genPos (), head: chance (0.6) ? null : genPos () };
            case "setSelections":
            {
               const n = 2 + ri (3), ranges = [];
               for (let i = 0; i < n; ++i) ranges .push ([genPos (), chance (0.6) ? null : genPos ()]);
               return { op: "setSelections", ranges, primary: ri (n) };
            }
            case "extending": return { op: "extending", val: chance (0.4) };
            case "tick": return { op: "tick", ms: pick ([10, 100, 300, 1300, 3000]) };
            case "copy": return { op: "copy" };
            case "cut": return { op: "cut" };
            case "paste": return chance (0.6) ? { op: "paste", text: null } : { op: "paste", text: pick (["a\nb", "x", "foo\nbar\nbaz", "\n", "1\n2"]) };
         }
      };

      // prepareCopyCut (src/input/TextareaInput.js) as a function of the editor.
      const copyCut = isCut =>
      {
         if (cm .somethingSelected ())
            __setLastCopied ({ lineWise: false, text: cm .getSelections () });
         else if (!cm .options .lineWiseCopyCut)
            return null;
         else
         {
            const ranges = __copyableRanges (cm);
            __setLastCopied ({ lineWise: true, text: ranges .text });
            if (isCut) cm .setSelections (ranges .ranges, null, { scroll: false });
            else return ranges .text .join ("\n");
         }
         const out = cm .getSelection ();
         if (isCut)
         {
            cm .state .cutIncoming = +new Date;
            cm .operation (() => __applyTextInput (cm, "", 0, null, null));
         }
         return out;
      };

      const apply = o =>
      {
         switch (o .op)
         {
            case "cmd": cm .execCommand (o .name); return null;
            case "shift": cm .display .shift = true; try { cm .execCommand (o .name); } finally { cm .display .shift = false; } return null;
            case "type": cm .operation (() => __applyTextInput (cm, o .text, 0, null, null)); return null;
            case "select": cm .setSelection (mk (o .anchor), o .head ? mk (o .head) : undefined); return null;
            case "addSelection": cm .addSelection (mk (o .anchor), o .head ? mk (o .head) : undefined); return null;
            case "setSelections": cm .setSelections (o .ranges .map (r => ({ anchor: mk (r [0]), head: mk (r [1] || r [0]) })), o .primary); return null;
            case "extending": cm .setExtending (o .val); return null;
            case "tick": window .__now += o .ms; return null;
            case "copy": { const t = copyCut (false); if (t != null) clipboard = t; return t == null ? t : hashText (t); }
            case "cut": { const t = copyCut (true); if (t != null) clipboard = t; return t == null ? t : hashText (t); }
            case "paste":
            {
               const t = o .text == null ? clipboard : o .text;
               if (t) cm .operation (() => __applyTextInput (cm, t, 0, null, "paste"));
               return null;
            }
         }
         throw new Error ("unknown op " + o .op);
      };

      const ops = [], out = [];
      // Initial selection.
      const first = chance (0.3) ? { op: "setSelections", ranges: [[genPos (), null], [genPos (), null], [genPos (), chance (0.5) ? genPos () : null]], primary: 0 }
         : { op: "select", anchor: genPos (), head: chance (0.3) ? genPos () : null };
      for (let k = 0; k < nsteps + 1; ++k)
      {
         const o = k == 0 ? first : genStep ();
         ops .push (o);
         let r, err = null;
         try
         {
            cm .getStateAfter (cm .lastLine (), true);
            r = apply (o);
         }
         catch (e) { err = String (e && e .message); }
         const s = SNAP ();
         if (r != null) s .push (r);
         if (err != null) s .push ({ err });
         out .push (s);
      }
      cm .getWrapperElement () .remove ();
      return { seed: spec .seed, init, ops, out };
   }

   return specs .map (runSeq);
}

await page .addScriptTag ({ content: runSequences .toString () });

const results = [];
const CHUNK = 25;
for (let i = 0; i < specs .length; i += CHUNK)
{
   const chunk = specs .slice (i, i + CHUNK);
   results .push (... await page .evaluate (([s, n]) => runSequences (s, n), [chunk, NSTEPS]));
}

await browser .close ();
server .close ();

let errors = 0, steps = 0;
for (const r of results) { steps += r .ops .length; for (const s of r .out) if (s .some (x => x && x .err)) ++errors; }
const corpusSeqs = results .filter (r => r .init .path) .length;

await writeFile (OUT, JSON .stringify ({ cm: "5.65.21", generator: "oracle/cm5-cmd-ops.mjs", steps: NSTEPS, corpusFiles: corpus .length, sequences: results }));
console .log (`${results .length} sequences (${corpusSeqs} from corpus-items, ${corpus .length} eligible files), ${steps} steps -> ${OUT} (${errors} steps threw in JS)`);
