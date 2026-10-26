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
   return out;
}

await page .addScriptTag ({ content: scenarios .toString () });
const result = await page .evaluate (() => scenarios ());
await browser .close ();
server .close ();

const json = JSON .stringify (result, null, 1) + "\n";
if (OUT) await writeFile (OUT, json); else process .stdout .write (json);
