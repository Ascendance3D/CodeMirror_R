/**
 * P10-01 editor recordings: mode/rust on a real CodeMirror 5 editor in headless
 * chromium. The Rust side is crates/cm-view/tests/rust_mode_editor.rs.
 *
 *   CM5_BUILD_DIR=<built copy of oracle/cm5-build> node oracle/cm5-rust-editor.mjs [out.json]
 *        default out: crates/cm-view/tests/data/cm5-rust-editor.json
 *
 * The vendored oracle/cm5-build is never built (README.md, "C0 suite"), so this script
 * reads lib/codemirror.js from a disposable built copy named by CM5_BUILD_DIR.
 *
 * Per document (indentUnit 4): getLineTokens(line, true) as [start, end, string,
 * type]; indentLine(line, "smart") on a fresh editor per line; indentLine "smart"
 * over every line in order; getModeAt / getMode().name. Electric input: the cursor at
 * a position, then a real "}" key press (the input poll runs triggerElectric).
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
const OUT = process .argv [2] || join (HERE, "..", "crates", "cm-view", "tests", "data", "cm5-rust-editor.json");

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
await page .goto (`http://127.0.0.1:${server .address () .port}/__harness.html`);
await page .addScriptTag ({ url: "/lib/codemirror.js" });
await page .waitForFunction (() => window .CodeMirror);
await page .addScriptTag ({ url: "/addon/mode/simple.js" });
await page .addScriptTag ({ url: "/mode/rust/rust.js" });

const flat = s => s .split ("\n") .map (l => l .replace (/^\s+/, "")) .join ("\n");
const demo = (await readFile (join (ROOT, "mode", "rust", "index.html"), "utf8"))
   .match (/<textarea id="code" name="code">\n?([\s\S]*?)<\/textarea>/) [1]
   .replace (/&lt;/g, "<") .replace (/&gt;/g, ">") .replace (/&amp;/g, "&");

const DOCS = [
   ["demo", "rust", demo],
   ["demo-flat", "rust", flat (demo)],
   ["blocks-flat", "text/x-rustsrc", flat ("fn f() {\n    /* a\n       b\n    */\n    let s = \"x\n  y\";\n    if x {\n        y\n    } else {\n        z\n    }\n}\n")],
   ["nested-flat", "text/rust", flat ("impl X {\n    fn g(&self) -> Vec<(u8, u8)> {\n        vec![\n            (1, 2),\n        ]\n    }\n}")],
   ["over-indented", "rust", "fn f() {\n                x\n        }\n  }\n)\n"],
   ["raw-and-attrs", "rust", "#[derive(Debug)]\nstruct S<'a> {\nr: &'a str, // c\n}\nlet r = r#\"\n{ not code\n\"#;\n{\n}"],
];

const docs = await page .evaluate (DOCS =>
{
   const out = [];
   for (const [name, mode, value] of DOCS)
   {
      const make = () => CodeMirror (document .body, { value, mode, indentUnit: 4 });
      let cm = make ();
      const tokens = [];
      for (let i = 0; i < cm .lineCount (); ++i)
         tokens .push (cm .getLineTokens (i, true) .map (t => [t .start, t .end, t .string, t .type]));
      const modeName = cm .getMode () .name, modeAt = cm .getModeAt (CodeMirror .Pos (0, 0)) .name;
      cm .getWrapperElement () .remove ();
      const each = [];
      for (let i = 0; i < value .split ("\n") .length; ++i)
      {
         cm = make ();
         cm .indentLine (i, "smart");
         each .push (cm .getLine (i));
         cm .getWrapperElement () .remove ();
      }
      cm = make ();
      for (let i = 0; i < cm .lineCount (); ++i) cm .indentLine (i, "smart");
      const all = cm .getValue ();
      cm .getWrapperElement () .remove ();
      out .push ({ name, mode, value, modeName, modeAt, tokens, each, all });
   }
   return out;
}, DOCS);

const ELECTRIC = [
   ["close-block", "fn f() {\n    if x {\n        y\n        ", [3, 8]],
   ["close-outer", "fn f() {\n    if x {\n        y\n    }\n        ", [4, 8]],
   ["not-alone", "fn f() {\n    x ", [1, 6]],
   ["in-comment", "fn f() {\n    /*\n            ", [2, 12]],
   ["in-string", "fn f() {\n    let s = \"\n            ", [2, 12]],
   ["no-indent-yet", "}", [0, 0]],
   ["line-start", "fn f() {\n    x\n", [2, 0]],
];

const electric = [];
for (const [name, value, [line, ch]] of ELECTRIC)
{
   await page .evaluate (([value, line, ch]) =>
   {
      document .querySelectorAll (".CodeMirror") .forEach (e => e .remove ());
      window .cm = CodeMirror (document .body, { value, mode: "rust", indentUnit: 4 });
      cm .focus ();
      cm .setCursor (CodeMirror .Pos (line, ch));
   }, [value, line, ch]);
   await page .keyboard .type ("}");
   await page .waitForTimeout (50);
   const r = await page .evaluate (() => ({ value_after: cm .getValue (), cursor: [cm .getCursor () .line, cm .getCursor () .ch] }));
   electric .push ({ name, value, at: [line, ch], ... r });
}

await browser .close ();
server .close ();
await writeFile (OUT, JSON .stringify ({ cm: "5.65.21", generator: "oracle/cm5-rust-editor.mjs", docs, electric }, null, 1) + "\n");
console .log (`${docs .length} documents, ${electric .length} electric cases -> ${OUT}`);
