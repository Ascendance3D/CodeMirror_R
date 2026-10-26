/**
 * P0 behavior recordings: small, named CodeMirror 5 scenarios run on a real editor in
 * headless chromium, for the behavior differences of the 2026-10-25 coverage audit
 * (08_COVERAGE_AUDIT.md, "Behavior differences"). Each scenario prints its recorded
 * state; the Rust tests that reproduce a scenario cite its name and copy its values.
 *
 *   CM5_BUILD_DIR=<built copy of oracle/cm5-build> node oracle/cm5-behavior-repro.mjs [--out file.json]
 *
 * The vendored oracle/cm5-build is never built (README.md, "C0 suite"), so this script
 * reads lib/codemirror.js from a disposable built copy named by CM5_BUILD_DIR.
 *
 * Measurement is the same as oracle/cm5-cmd-ops.mjs: DejaVu Sans Mono 32px, 38px lines,
 * a 900x500 editor in a 1200x900 page, so the Rust replay uses `Metrics::oracle()`.
 */
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = normalize (join (fileURLToPath (import.meta.url), ".."));
const ROOT = normalize (process .env .CM5_BUILD_DIR || "");
if (!process .env .CM5_BUILD_DIR) throw new Error ("set CM5_BUILD_DIR to a built copy of oracle/cm5-build");
const { chromium } = createRequire (process .env .CM5_PLAYWRIGHT_FROM || join (HERE, "../../../x_ite-tests/")) ("playwright");

const args = process .argv .slice (2);
const outAt = args .indexOf ("--out");
const OUT = outAt >= 0 ? args [outAt + 1] : null;

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
   const path = normalize (join (ROOT, decodeURIComponent (url .pathname)));
   if (!path .startsWith (ROOT)) { res .writeHead (403); return res .end (); }
   try
   {
      const body = await readFile (path);
      res .writeHead (200, { "Content-Type": { ".js": "text/javascript", ".css": "text/css" } [extname (path)] || "application/octet-stream" });
      res .end (body);
   }
   catch { res .writeHead (404); res .end (); }
});

await new Promise (r => server .listen (0, "127.0.0.1", r));
const browser = await chromium .launch ({ headless: true });
const page    = await browser .newPage ({ viewport: { width: 1200, height: 900 } });
await page .goto (`http://127.0.0.1:${server .address () .port}/__harness.html`);
await page .addScriptTag ({ url: "/lib/codemirror.js" });
await page .waitForFunction (() => window .CodeMirror);
for (const m of ["xml", "javascript", "css", "htmlmixed"]) await page .addScriptTag ({ url: `/mode/${m}/${m}.js` });

// ---------------------------------------------------------------------------------
// Everything below runs in the page.

function scenarios ()
{
   const Pos = CodeMirror .Pos;
   const lines = n => Array .from ({ length: n }, (_, i) => "line " + i) .join ("\n");
   const make = () => CodeMirror (document .body, { value: lines (200) });
   const state = cm => ({
      scrollTop: cm .getScrollInfo () .top,
      cursor: { line: cm .getCursor () .line, ch: cm .getCursor () .ch },
      sels: cm .listSelections () .map (r => [r .anchor .line, r .anchor .ch, r .head .line, r .head .ch]),
   });
   const out = {};
   const run = (name, setup, act) =>
   {
      const cm = make ();
      if (cm .defaultCharWidth () !== 19.265625 || cm .defaultTextHeight () !== 38)
         throw new Error ("metrics differ: " + cm .defaultCharWidth () + " " + cm .defaultTextHeight ());
      setup (cm);
      const before = state (cm);
      act (cm);
      out [name] = { before, after: state (cm) };
      cm .getWrapperElement () .remove ();
   };
   // G13 #1: replaceSelections without a collapse argument (or with "end") calls
   // ensureCursorVisible (src/model/Doc.js:184).
   const far = cm => cm .setCursor (Pos (150, 0), null, { scroll: false });
   run ("replaceSelection_noCollapse", far, cm => cm .replaceSelection ("x"));
   run ("replaceSelection_end",        far, cm => cm .replaceSelection ("x", "end"));
   run ("replaceSelection_around",     far, cm => cm .replaceSelection ("x", "around"));
   run ("replaceSelection_start",      far, cm => cm .replaceSelection ("x", "start"));
   run ("replaceSelections_multi",
        cm => cm .setSelections ([{ anchor: Pos (10, 0) }, { anchor: Pos (150, 0) }], 1, { scroll: false }),
        cm => cm .replaceSelections (["a", "b"]));
   // Control: replaceRange does not scroll (makeChange -> setSelectionNoUndo(sel_dontScroll)).
   run ("replaceRange_control", far, cm => cm .replaceRange ("x", Pos (150, 0)));

   // G13 #2/#3: the viewport after scrolls (endOperation_R1 computes the update for the
   // target scrollTop; updateDisplayIfNeeded bails out when the visible lines are
   // already rendered).
   {
      const cm = make ();
      const vp = () => { const v = cm .getViewport (); return [v .from, v .to, cm .getScrollInfo () .top]; };
      const steps = [["start", vp ()]];
      for (const y of [38, 76, 380, 760, 1900, 1862, 0, 3000, 7600, 7000])
      {
         cm .scrollTo (null, y);
         steps .push (["scrollTo " + y, vp ()]);
      }
      cm .operation (() => { cm .scrollTo (null, 3800); cm .scrollIntoView (Pos (20, 0)); });
      steps .push (["op scrollTo 3800 + scrollIntoView 20", vp ()]);
      cm .operation (() => { cm .scrollIntoView (Pos (180, 0)); cm .scrollTo (null, 100); });
      steps .push (["op scrollIntoView 180 + scrollTo 100", vp ()]);
      out .viewport_scrolls = steps;
      cm .getWrapperElement () .remove ();
   }

   // G13 #5: scrollTo inside an operation is applied at its end.
   {
      const cm = make ();
      let inside;
      cm .operation (() => { cm .scrollTo (null, 1000); inside = cm .getScrollInfo () .top; });
      out .scrollTo_inOperation = { inside, after: cm .getScrollInfo () .top };
      cm .getWrapperElement () .remove ();
   }

   // G13 #10: scroll changes below 2px.
   {
      const cm = CodeMirror (document .body, { value: lines (200) + "\n" + "x" .repeat (300) });
      const s = () => [cm .getScrollInfo () .top, cm .getScrollInfo () .left];
      const steps = [];
      for (const [x, y] of [[null, 1], [null, 2], [null, 3], [1, null], [2, null], [3, null], [4.5, null], [null, 4.5]])
      {
         cm .scrollTo (x, y);
         steps .push ([x, y, s ()]);
      }
      out .scroll_small = steps;
      cm .getWrapperElement () .remove ();
   }

   // G13 #9: a marker "clear" handler is delivered by signalLater, at the end of the
   // operation that cleared it.
   {
      const cm = make ();
      const log = [];
      const m = cm .markText (Pos (1, 0), Pos (1, 4));
      m .on ("clear", (from, to) => log .push (["clear", from .line, from .ch, to .line, to .ch, cm .getLine (1)]));
      cm .operation (() =>
      {
         m .clear ();
         log .push (["after clear"]);
         cm .replaceRange ("X", Pos (1, 0));
         log .push (["after edit"]);
      });
      log .push (["after operation"]);
      const m2 = cm .markText (Pos (2, 0), Pos (2, 4));
      m2 .on ("clear", () => log .push (["clear2"]));
      m2 .clear ();
      log .push (["after clear2 outside operation"]);
      out .marker_clear_order = log;
      cm .getWrapperElement () .remove ();
   }

   // P0 review of the G13 #1 fix: replaceSelections inside a larger operation, near the
   // end of a large document, over a read-only span, its history, and on a detached Doc.
   const runWith = (name, value, setup, act) =>
   {
      const cm = CodeMirror (document .body, { value });
      setup (cm);
      const before = state (cm);
      act (cm);
      out [name] = { before, after: state (cm) };
      cm .getWrapperElement () .remove ();
   };
   run ("replaceSelection_inOp_thenSetCursorNoScroll", far,
        cm => cm .operation (() => { cm .replaceSelection ("x"); cm .setCursor (Pos (10, 0), null, { scroll: false }); }));
   run ("replaceSelection_inOp_thenSetCursor", far,
        cm => cm .operation (() => { cm .replaceSelection ("x"); cm .setCursor (Pos (10, 0)); }));
   run ("replaceSelection_inOp_twice", far,
        cm => cm .operation (() =>
        {
           cm .replaceSelection ("x");
           cm .setCursor (Pos (40, 0), null, { scroll: false });
           cm .replaceSelection ("y");
        }));
   runWith ("replaceSelection_largeDocEnd", lines (5000),
            cm => cm .setCursor (Pos (4999, 9), null, { scroll: false }),
            cm => cm .replaceSelection ("x\ny\nz"));
   run ("replaceSelection_readOnlySpan",
        cm => { cm .markText (Pos (150, 0), Pos (150, 4), { readOnly: true }); cm .setCursor (Pos (150, 2), null, { scroll: false }); },
        cm => cm .replaceSelection ("x"));
   {
      const cm = make ();
      far (cm);
      cm .clearHistory ();
      cm .replaceSelection ("x");
      const kinds = () => cm .getHistory () .done .map (e => e .changes ? "changes" : "selection");
      const done = kinds (), size = cm .historySize ();
      cm .undoSelection ();
      out .replaceSelection_history = { done, size, afterUndoSelection: state (cm), doneAfter: kinds () };
      cm .getWrapperElement () .remove ();
   }
   {
      const doc = CodeMirror .Doc (lines (200));
      doc .setCursor (Pos (150, 0));
      doc .replaceSelection ("x");
      out .replaceSelection_detached = { line150: doc .getLine (150), cursor: doc .getCursor (), size: doc .historySize () };
   }

   // G13 #11: cursor width from lib/codemirror.css (normal and overwrite).
   {
      const cm = make ();
      cm .setCursor (Pos (1, 2));
      const measure = () =>
      {
         const c = cm .display .cursorDiv .querySelector (".CodeMirror-cursor");
         const st = getComputedStyle (c);
         return { width: c .getBoundingClientRect () .width, borderLeft: st .borderLeftWidth, borderRight: st .borderRightWidth,
                  cssWidth: st .width, devicePixelRatio: window .devicePixelRatio };
      };
      const normal = measure ();
      cm .toggleOverwrite (true);
      out .cursor_width = { normal, overwrite: measure (), charWidth: cm .defaultCharWidth () };
      cm .getWrapperElement () .remove ();
   }

   // G13 #12: StringStream.backUp past the start; every later read is recorded.
   {
      const trace = [];
      const s = new CodeMirror .StringStream ("abcdef", 4);
      s .next ();
      s .start = s .pos;
      s .backUp (3);
      const look = op => { let r = op (); trace .push ([op .toString () .replace (/^\(\) => /, ""), r === undefined ? "undefined" : r, s .pos, s .start]); };
      look (() => s .pos);
      look (() => s .current ());
      look (() => s .peek ());
      look (() => s .eol ());
      look (() => s .sol ());
      look (() => s .column ());
      look (() => s .match ("ef", false));
      look (() => s .match (/^e/, false) && "matched");
      look (() => s .next ());
      look (() => s .next ());
      look (() => s .current ());
      look (() => s .eat ("a"));
      look (() => s .next ());
      look (() => s .current ());
      out .stringStream_backUp = trace;
   }

   // G13 #13: htmlmixed looks up the inner mode when the tag closes (CodeMirror.getMode
   // at token time), so a MIME defined after getMode("htmlmixed") still applies.
   {
      const tokens = (mode, text) =>
      {
         const st = CodeMirror .startState (mode), res = [];
         const stream = new CodeMirror .StringStream (text, 2);
         while (!stream .eol ())
         {
            const style = mode .token (stream, st);
            res .push ([stream .current (), style]);
            stream .start = stream .pos;
         }
         return res;
      };
      const spec = { name: "htmlmixed", tags: { script: [["type", /^text\/x-late$/, "text/x-late"], [null, null, "javascript"]] } };
      const text = '<script type="text/x-late">var a</script>';
      const before = tokens (CodeMirror .getMode ({ indentUnit: 2 }, spec), text);
      const mode = CodeMirror .getMode ({ indentUnit: 2 }, spec);
      CodeMirror .defineMIME ("text/x-late", "javascript");
      out .htmlmixed_lateMime = { undefinedMime: before, definedAfterGetMode: tokens (mode, text) };
   }
   return out;
}

// G13 #4: a key sequence stays active until a 50 ms timer clears it (key_events.js:48-60).
async function keySequence ()
{
   const Pos = CodeMirror .Pos;
   const lines = Array .from ({ length: 200 }, (_, i) => "line " + i) .join ("\n");
   const cm = CodeMirror (document .body, { value: lines, extraKeys: CodeMirror .normalizeKeyMap ({ "Ctrl-K Ctrl-X": "goDocEnd", "Ctrl-X": "goDocStart" }) });
   const key = keyCode => cm .triggerOnKeyDown ({ type: "keydown", keyCode, ctrlKey: true, shiftKey: false, altKey: false, metaKey: false,
                                                 preventDefault () {}, stopPropagation () {} });
   const cur = () => [cm .getCursor () .line, cm .getCursor () .ch, cm .state .keySeq];
   const sleep = ms => new Promise (r => setTimeout (r, ms));
   const log = [];
   cm .setCursor (Pos (5, 0));
   key (75); log .push (["Ctrl-K", cur ()]);
   key (88); log .push (["Ctrl-X", cur ()]);
   cm .setCursor (Pos (5, 0));
   key (88); log .push (["Ctrl-X within 50ms", cur ()]);
   cm .setCursor (Pos (5, 0));
   await sleep (120);
   log .push (["after 120ms", cur ()]);
   key (88); log .push (["Ctrl-X after 120ms", cur ()]);
   cm .getWrapperElement () .remove ();
   return log;
}

await page .addScriptTag ({ content: scenarios .toString () });
const result = await page .evaluate (() => scenarios ());
await page .addScriptTag ({ content: keySequence .toString () });
result .key_sequence_timeout = await page .evaluate (() => keySequence ());

// Mouse scenarios use real input (playwright), each on a fresh page. The editor is at
// the page origin, so client y = 4 (top padding) + document y.
async function freshPage (scale = 1)
{
   const p = await browser .newPage ({ viewport: { width: 1200, height: 900 }, deviceScaleFactor: scale });
   await p .goto (`http://127.0.0.1:${server .address () .port}/__harness.html`);
   await p .addScriptTag ({ url: "/lib/codemirror.js" });
   await p .evaluate (() =>
   {
      window .cm = CodeMirror (document .body, { value: Array .from ({ length: 200 }, (_, i) => "line " + i) .join ("\n") });
      window .st = () => ({ scrollTop: cm .getScrollInfo () .top,
                            sels: cm .listSelections () .map (r => [r .anchor .line, r .anchor .ch, r .head .line, r .head .ch]) });
   });
   return p;
}
const xAt = ch => 4 + ch * 19.265625 + 1;   // inside column `ch`
const yAt = line => 4 + line * 38 + 19;     // middle of document line `line`

// G13 #7: alt-drag (rectangle) calls cm.scrollIntoView(pos) (mouse_events.js:265).
// The target y = 499 is on line 13 (document y 495), partly visible in the 500px editor.
for (const [name, alt] of [["rectangle_select_scroll", true], ["rectangle_select_scroll_control_noAlt", false]])
{
   const p = await freshPage ();
   if (alt) await p .keyboard .down ("Alt");
   await p .mouse .move (xAt (2), yAt (2));
   await p .mouse .down ();
   await p .mouse .move (xAt (4), 499, { steps: 4 });
   const moved = await p .evaluate (() => st ());
   await p .waitForTimeout (300);
   const later = await p .evaluate (() => st ());
   await p .mouse .up ();
   if (alt) await p .keyboard .up ("Alt");
   result [name] = { moved, after300ms: later };
   await p .close ();
}

// G13 #8: a drag held below the editor keeps scrolling on timers (150 ms re-extend,
// then 20 px every 50 ms, mouse_events.js:297-307).
{
   const p = await freshPage ();
   await p .mouse .move (xAt (2), yAt (2));
   await p .mouse .down ();
   await p .mouse .move (xAt (2), 560, { steps: 4 });
   const samples = [];
   const t0 = Date .now ();
   for (const at of [0, 100, 200, 400, 800])
   {
      const wait = at - (Date .now () - t0);
      if (wait > 0) await p .waitForTimeout (wait);
      samples .push ([at, await p .evaluate (() => st ())]);
   }
   await p .mouse .up ();
   result .drag_autoscroll_held = samples;
   await p .close ();
}

// G13 #6: an internal drag-move drop uses setSelectionNoUndo (drop_events.js:77).
{
   const p = await freshPage ();
   await p .evaluate (() => { cm .setSelection (CodeMirror .Pos (1, 0), CodeMirror .Pos (1, 4)); cm .clearHistory (); });
   await p .mouse .move (xAt (2), yAt (1));
   await p .mouse .down ();
   await p .mouse .move (xAt (6), yAt (5), { steps: 8 });
   await p .mouse .up ();
   await p .waitForTimeout (50);
   result .drop_history = await p .evaluate (() =>
   {
      const kinds = () => cm .getHistory () .done .map (e => e .changes ? "changes" : "selection");
      const selEvents = () => cm .getHistory () .done .filter (e => !e .changes)
         .map (e => e .ranges .map (r => [r .anchor .line, r .anchor .ch, r .head .line, r .head .ch]));
      const after = { ...st (), selEvents: selEvents (), line1: cm .getLine (1), line5: cm .getLine (5), done: kinds () };
      cm .undoSelection ();
      const undoSel = { ...st (), done: kinds (), selEvents: selEvents () };
      cm .undoSelection ();
      const undoSel2 = { ...st (), done: kinds () };
      cm .undo ();
      const undo = { ...st (), line1: cm .getLine (1), line5: cm .getLine (5) };
      return { after, undoSel, undoSel2, undo };
   });
   await p .close ();
}

// G13 #11 at device scale 2.
{
   const p = await freshPage (2);
   result .cursor_width_scale2 = await p .evaluate (() =>
   {
      cm .setCursor (CodeMirror .Pos (1, 2));
      const c = cm .display .cursorDiv .querySelector (".CodeMirror-cursor");
      return { width: c .getBoundingClientRect () .width, borderLeft: getComputedStyle (c) .borderLeftWidth, devicePixelRatio: window .devicePixelRatio };
   });
   await p .close ();
}
await browser .close ();
server .close ();

const json = JSON .stringify (result, null, 1) + "\n";
if (OUT) await writeFile (OUT, json); else process .stdout .write (json);
