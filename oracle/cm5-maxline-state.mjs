/**
 * L-A recordings: the longest line (`display.maxLine`) and the parser states its
 * measurement leaves, on a real CodeMirror 5 editor in headless chromium. The Rust
 * side is crates/cm-view/tests/maxline_state.rs.
 *
 *   CM5_BUILD_DIR=<built copy of oracle/cm5-build> node oracle/cm5-maxline-state.mjs [out.json]
 *        default out: crates/cm-view/tests/data/cm5-maxline-state.json
 *
 * The vendored oracle/cm5-build is never built (README.md, "C0 suite"), so this script
 * reads lib/codemirror.js from a disposable built copy named by CM5_BUILD_DIR.
 *
 * Each case builds an editor (indentUnit 4) and runs its steps, one operation each.
 * After construction and after every step it records the rendered range, the
 * maxLine bookkeeping (`getLineNumber(display.maxLine)`, `maxLineLength`,
 * `maxLineChanged`), the lines with a saved state (`stateAfter`), the lines with
 * current styles (`styles[0] == modeGen`), and both frontiers. `fold` is a collapsed
 * `markText`, `unfold` clears the last one. Steps resolve their
 * positions in the page ("end" = the line's length) and record the concrete ones;
 * undo and redo record the range `makeChangeFromHistory` scrolls into view.
 * Everything runs in one synchronous evaluate, so the highlight worker (a 400 ms
 * timeout) never runs between steps.
 */
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { extname, join, normalize } from "node:path";
import { safeResolve } from "./server-utils.mjs";
import { fileURLToPath } from "node:url";

const HERE = normalize (join (fileURLToPath (import.meta.url), ".."));
const ROOT = normalize (process .env .CM5_BUILD_DIR || "");
if (!process .env .CM5_BUILD_DIR) throw new Error ("set CM5_BUILD_DIR to a built copy of oracle/cm5-build");
const { chromium } = createRequire (process .env .CM5_PLAYWRIGHT_FROM || join (HERE, "../../../x_ite-tests/")) ("playwright");
const OUT = process .argv [2] || join (HERE, "..", "crates", "cm-view", "tests", "data", "cm5-maxline-state.json");

const HARNESS = `<!doctype html><meta charset="utf-8"><link rel="stylesheet" href="/lib/codemirror.css">`;

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
      res .writeHead (200, { "Content-Type": { ".js": "text/javascript", ".css": "text/css" } [extname (path)] || "application/octet-stream" });
      res .end (body);
   }
   catch { res .writeHead (404); res .end (); }
});

await new Promise (r => server .listen (0, "127.0.0.1", r));
const browser = await chromium .launch ({ headless: true });
const page    = await browser .newPage ({ viewport: { width: 1200, height: 900 } });
async function freshPage ()
{
   await page .goto (`http://127.0.0.1:${server .address () .port}/__harness.html`);
   await page .addScriptTag ({ url: "/lib/codemirror.js" });
   await page .waitForFunction (() => window .CodeMirror);
   await page .addScriptTag ({ url: "/mode/clike/clike.js" });
   await page .addScriptTag ({ url: "/addon/mode/simple.js" });
   await page .addScriptTag ({ url: "/mode/rust/rust.js" });
}

// A C document of `n` lines in blocks of six, with varied indentation; `long` maps a
// line number to a replacement text.
function cdoc (n, long = {})
{
   const lines = [];
   for (let i = 0; lines .length < n; ++i)
      lines .push (`int f${i}(int x) {`, `    if (x > ${i}) {`, `        y = ${i};`, `    }`, `    return x;`, `}`);
   lines .length = n;
   for (const [k, v] of Object .entries (long)) lines [k] = v;
   return lines .join ("\n");
}
function rustdoc (n, long = {})
{
   const lines = [];
   for (let i = 0; lines .length < n; ++i)
      lines .push (`fn f${i}(x: i32) -> i32 {`, `    if x > ${i} {`, `        return ${i};`, `    }`, `    x`, `}`);
   lines .length = n;
   for (const [k, v] of Object .entries (long)) lines [k] = v;
   return lines .join ("\n");
}
const longLine = (indent, len, tag) => { const s = " " .repeat (indent) + `y = ${tag}; // `; return s + "x" .repeat (len - s .length); };

const C = "text/x-csrc";
const CASES = [
   ["far-c", C, cdoc (300, { 200: longLine (8, 80, 200) }), [
      { op: "replace", text: "a", from: [10, 0] },                        // in view, elsewhere
      { op: "replace", text: "b", from: [150, "end"] },                   // off view, elsewhere
      { op: "replace", text: "c", from: [200, "end"] },                   // touches the max line
      { op: "replace", text: longLine (4, 100, 250), from: [250, 0], to: [250, "end"] }, // a new longest line
      { op: "replace", text: longLine (4, 100, 100), from: [100, 0], to: [100, "end"] }, // equal length
      { op: "replace", text: "    q;", from: [250, 0], to: [250, "end"] }, // shortens the max line
      { op: "undo" },
      { op: "redo" },
      { op: "replace", text: "\n\n", from: [5, 0] },                      // lines before it
      { op: "replace", text: "", from: [102, 0], to: [103, 0] },          // deletes the max line
      { op: "scroll", line: 190 },
      { op: "scroll", line: 0 },
      { op: "option", name: "tabSize", value: 8 },
      { op: "option", name: "mode", value: C },
      { op: "option", name: "lineWrapping", value: true },
      { op: "option", name: "lineWrapping", value: false },
      { op: "indent", line: 205 },
      { op: "indent", line: 206 },
      { op: "indent", line: 207 },
      { op: "setValue", text: cdoc (400, { 350: longLine (8, 90, 350) }) },
      { op: "replace", text: "d", from: [350, "end"] },
   ]],
   ["inview-c", C, cdoc (300, { 3: longLine (4, 60, 3) }), [
      { op: "replace", text: longLine (4, 61, 150), from: [150, 0], to: [150, "end"] },
      { op: "replace", text: longLine (4, 62, 160), from: [160, 0], to: [160, "end"] },
      { op: "replace", text: longLine (4, 63, 170), from: [170, 0], to: [170, "end"] },
      { op: "replace", text: "e", from: [3, "end"] },
   ]],
   ["equal-c", C, cdoc (300, { 120: longLine (8, 70, 120), 200: longLine (8, 70, 200) }), [
      { op: "replace", text: "", from: [120, 10], to: [120, 20] },
      { op: "replace", text: "f", from: [120, "end"] },
      { op: "scroll", line: 110 },
      { op: "replace", text: "0123456789012345678901234567890", from: [120, "end"] },
   ]],
   ["tabs-c", C, cdoc (300, { 5: "\t\t\t\t\t\t\t\t\t\t" + "z = 5; // abcdefgh", 200: longLine (8, 45, 200) }), [
      { op: "replace", text: "g", from: [200, "end"] },
      { op: "option", name: "tabSize", value: 2 },
   ]],
   ["empty", C, "", [
      { op: "replace", text: "int x;", from: [0, 0] },
   ]],
   ["single", C, "int main() { return 0; }", [
      { op: "replace", text: " ", from: [0, "end"] },
   ]],
   ["short-c", C, cdoc (12, { 7: longLine (8, 50, 7) }), [
      { op: "replace", text: "h", from: [7, "end"] },
      { op: "indent", line: 8 },
   ]],
   ["large-c", C, cdoc (5000, { 4000: longLine (8, 100, 4000) }), [
      { op: "replace", text: "i", from: [2000, "end"] },
      { op: "replace", text: "j", from: [4000, "end"] },
      { op: "indent", line: 4001 },
   ]],
   ["fold-c", C, cdoc (300, { 200: longLine (8, 80, 200), 240: longLine (8, 70, 240) }), [
      { op: "fold", from: [150, 3], to: [205, 2] },                       // hides the max line
      { op: "unfold" },
      { op: "fold", from: [100, 3], to: [120, 2] },                       // elsewhere
      { op: "unfold" },
   ]],
   ["rust-far", "rust", rustdoc (300, { 220: longLine (8, 80, 220) }), [
      { op: "indent", line: 221 },
      { op: "indent", line: 222 },
      { op: "replace", text: "k", from: [220, "end"] },
      { op: "indent", line: 223 },
   ]],
];

const cases = [];
for (const [name, mode, value, steps] of CASES)
{
   await freshPage ();
   cases .push (await page .evaluate (([name, mode, value, steps]) =>
   {
      const Pos = CodeMirror .Pos;
      const make = () => CodeMirror (document .body, { value, mode, indentUnit: 4 });
      function snap (cm)
      {
         const d = cm .display, doc = cm .doc, gen = cm .state .modeGen, states = [], styled = [];
         doc .iter (line =>
         {
            const n = doc .getLineNumber (line);
            if (line .stateAfter) states .push (n);
            if (line .styles && line .styles [0] == gen) styled .push (n);
         });
         return {
            view: [d .viewFrom, d .viewTo],
            maxLine: d .maxLine ? doc .getLineNumber (d .maxLine) : null,
            maxLineLength: d .maxLineLength,
            maxLineChanged: d .maxLineChanged,
            states, styled,
            highlightFrontier: doc .highlightFrontier,
            modeFrontier: doc .modeFrontier,
            lines: doc .lineCount (),
         };
      }
      const marks = [];
      const resolve = (cm, [l, c]) => [l, c == "end" ? cm .getLine (l) .length : c];
      function run (cm, s)
      {
         const r = { ... s };
         switch (s .op)
         {
            case "replace":
               r .from = resolve (cm, s .from);
               r .to = s .to ? resolve (cm, s .to) : r .from;
               cm .replaceRange (s .text, Pos (... r .from), Pos (... r .to));
               break;
            case "undo": case "redo":
            {
               // makeChangeFromHistory scrolls to its last change (changes.js:144).
               const last = (_, c) => r .scroll = [[c .from .line, c .from .ch], [c .from .line + c .text .length - 1,
                  c .text [c .text .length - 1] .length + (c .text .length == 1 ? c .from .ch : 0)]];
               cm .on ("change", last);
               cm [s .op] ();
               cm .off ("change", last);
               break;
            }
            case "setValue": cm .setValue (s .text); break;
            case "option": cm .setOption (s .name, s .value); break;
            case "scroll": cm .scrollTo (null, cm .heightAtLine (s .line, "local")); break;
            case "indent": cm .indentLine (s .line, "smart"); r .result = cm .getLine (s .line); break;
            case "fold":
               r .from = resolve (cm, s .from);
               r .to = resolve (cm, s .to);
               marks .push (cm .markText (Pos (... r .from), Pos (... r .to), { collapsed: true }));
               break;
            case "unfold": marks .pop () .clear (); break;
         }
         r .after = snap (cm);
         return r;
      }
      const cm = make ();
      const initial = snap (cm);
      const textHeight = cm .defaultTextHeight ();
      const done = steps .map (s => run (cm, s));
      cm .getWrapperElement () .remove ();
      // Repeated construction on the same page gives the same state.
      const again = make ();
      const repeat = snap (again);
      again .getWrapperElement () .remove ();
      return { name, mode, value, textHeight, initial, steps: done, repeat };
   }, [name, mode, value, steps]));
}

await browser .close ();
server .close ();
await writeFile (OUT, JSON .stringify ({ cm: "5.65.21", generator: "oracle/cm5-maxline-state.mjs", cases }, null, 1) + "\n");
console .log (`${cases .length} cases -> ${OUT}`);
