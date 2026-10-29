/**
 * P10-07 editor recordings: mode/clike on a real CodeMirror 5 editor in headless
 * chromium. The Rust side is crates/cm-view/tests/clike_mode_editor.rs.
 *
 *   CM5_BUILD_DIR=<built copy of oracle/cm5-build> node oracle/cm5-clike-editor.mjs [out.json]
 *        default out: crates/cm-view/tests/data/cm5-clike-editor.json
 *
 * The vendored oracle/cm5-build is never built (README.md, "C0 suite"), so this script
 * reads lib/codemirror.js from a disposable built copy named by CM5_BUILD_DIR.
 *
 * Per document (indentUnit 4): getLineTokens(line, true) as [start, end, string,
 * type]; indentLine(line, "smart") on a fresh editor per line; indentLine "smart"
 * over every line in order; getModeAt / getMode().name; getHelpers(pos, "hintWords").
 * Electric input: the cursor at a position, then real key presses, one per character
 * (the input poll runs triggerElectric after each).
 *
 * `view` is the rendered line range after construction: lines in it get saved states,
 * which findStartLine (non-precise, as indentLine "smart" uses) starts from. The Rust
 * test sizes its editor so the same lines are rendered.
 *
 * Every document is loaded on a fresh page, so Ceylon's module-scope stringTokenizer
 * starts null for each, as it does for each Rust editor (each has its own Registry).
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
const OUT = process .argv [2] || join (HERE, "..", "crates", "cm-view", "tests", "data", "cm5-clike-editor.json");

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
}

const flat = s => s .split ("\n") .map (l => l .replace (/^\s+/, "")) .join ("\n");
const demos = [];
for (const [file, re] of [["index.html", /<textarea id="([a-z]+)-code">\n?([\s\S]*?)<\/textarea>/g], ["scala.html", /<textarea id="(code)" name="code">\n?([\s\S]*?)<\/textarea>/g]])
   for (const m of (await readFile (join (ROOT, "mode", "clike", file), "utf8")) .matchAll (re))
      demos .push ([file == "scala.html" ? "scala-page" : m [1],
                    m [2] .replace (/&lt;/g, "<") .replace (/&gt;/g, ">") .replace (/&quot;/g, "\"") .replace (/&amp;/g, "&")]);
const DEMO_MIME = { c: "text/x-csrc", cpp: "text/x-c++src", objectivec: "text/x-objectivec", java: "text/x-java", scala: "text/x-scala",
                    kotlin: "text/x-kotlin", ceylon: "text/x-ceylon", "scala-page": "text/x-scala" };

const DOCS = [];
for (const [name, text] of demos)
{
   DOCS .push ([name, DEMO_MIME [name], text]);
   DOCS .push ([name + "-flat", DEMO_MIME [name], flat (text)]);
}
DOCS .push (
   ["switch-flat", "text/x-csrc", flat ("int f(int x) {\n  switch (x) {\n    case 1:\n      return 2;\n    default:\n      g(a,\n        b);\n  }\n  /* c\n     d */\n  return x +\n    1;\n}\n")],
   ["csharp-flat", "text/x-csharp", flat ("namespace N {\n  class C {\n    var s = @\"a\n\"\"b\";\n    void M() {\n      foreach (var x in y) {\n        z();\n      }\n    }\n  }\n}")],
   ["shader-flat", "x-shader/x-fragment", flat ("void main() {\n  vec4 c = vec4(1.0);\n  if (c.r > 0.5)\n    discard;\n  gl_FragColor = c;\n}")],
   ["nesc-flat", "text/x-nesc", flat ("module M {\n  uses interface I;\n}\nimplementation {\n  event void I.e() {\n    call T.start();\n  }\n}")],
   ["squirrel-flat", "text/x-squirrel", flat ("class A {\n  constructor() {\n    foreach (i in x) {\n      y();\n    }\n  }\n}")],
   ["objcpp-flat", "text/x-objectivec++", flat ("@interface A\n- (void)m;\n@end\ntemplate <class T>\nclass B {\n  T t;\n};")],
   ["cpp-hdr", "text/x-c++hdr", "namespace a {\nclass B {\n public:\n  B();\n};\n}\n"],
   ["c-hdr", "text/x-chdr", "#ifndef X\n#define X\nstruct s {\nint a;\n};\n#endif\n"],
   ["c-alias", "text/x-c", "int main() {\nreturn 0;\n}\n"],
   ["vertex", "x-shader/x-vertex", "attribute vec3 p;\nvoid main() {\ngl_Position = vec4(p, 1.0);\n}\n"],
   ["over-indented", "text/x-java", "class A {\n                x();\n        }\n  }\n)\n"],
   ["kotlin-chain", "text/x-kotlin", "fun f() {\nlist\n.map { it }\n.filter {\nit > 0\n}\nval x = a +\nb\n}\n"],
);

const docs = [];
for (const [name, mode, value] of DOCS)
{
   await freshPage ();
   docs .push (await page .evaluate (([name, mode, value]) =>
   {
      const make = () => CodeMirror (document .body, { value, mode, indentUnit: 4 });
      let cm = make ();
      const view = [cm .display .viewFrom, cm .display .viewTo];
      const tokens = [];
      for (let i = 0; i < cm .lineCount (); ++i)
         tokens .push (cm .getLineTokens (i, true) .map (t => [t .start, t .end, t .string, t .type]));
      const modeName = cm .getMode () .name, modeAt = cm .getModeAt (CodeMirror .Pos (0, 0)) .name;
      const hintWords = cm .getHelpers (CodeMirror .Pos (0, 0), "hintWords");
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
      return { name, mode, value, view, modeName, modeAt, hintWords, tokens, each, all };
   }, [name, mode, value]));
}

const ELECTRIC = [
   ["close-block", "text/x-csrc", "int f() {\n    if (x) {\n        y;\n        ", [3, 8], "}"],
   ["close-outer", "text/x-csrc", "int f() {\n    if (x) {\n        y;\n    }\n        ", [4, 8], "}"],
   ["open-brace", "text/x-csrc", "int f()\n        ", [1, 8], "{"],
   ["empty-braces", "text/x-java", "class A {\n        ", [1, 8], "{}"],
   ["case", "text/x-csrc", "int f() {\n    switch (x) {\n        a;\n            ", [3, 12], "case 1:"],
   ["default", "text/x-csrc", "int f() {\n    switch (x) {\n        case 1:\n            a;\n            ", [4, 12], "default:"],
   ["scala-case-no-electric", "text/x-scala", "object A {\n  x match {\n    case 1 => a\n        ", [3, 8], "case 2:"],
   ["scala-close", "text/x-scala", "object A {\n  def f = {\n    x\n        ", [3, 8], "}"],
   ["kotlin-close", "text/x-kotlin", "fun f() {\n    if (x) {\n        y\n            ", [3, 12], "}"],
   ["not-alone", "text/x-csrc", "int f() {\n    x ", [1, 6], "}"],
   ["in-comment", "text/x-csrc", "int f() {\n    /*\n            ", [2, 12], "}"],
   ["in-string", "text/x-java", "class A {\n    String s = \"\n            ", [2, 12], "}"],
   ["preprocessor-cont", "text/x-csrc", "#define X \\\n        ", [1, 8], "}"],
   ["top-level", "text/x-csrc", "}", [0, 0], "}"],
];

const electric = [];
for (const [name, mode, value, [line, ch], keys] of ELECTRIC)
{
   await freshPage ();
   await page .evaluate (([value, mode, line, ch]) =>
   {
      window .cm = CodeMirror (document .body, { value, mode, indentUnit: 4 });
      cm .focus ();
      cm .setCursor (CodeMirror .Pos (line, ch));
   }, [value, mode, line, ch]);
   for (const k of keys)
   {
      await page .keyboard .type (k);
      await page .waitForTimeout (30);
   }
   const r = await page .evaluate (() => ({ value_after: cm .getValue (), cursor: [cm .getCursor () .line, cm .getCursor () .ch] }));
   electric .push ({ name, mode, value, at: [line, ch], keys, ... r });
}

await browser .close ();
server .close ();
await writeFile (OUT, JSON .stringify ({ cm: "5.65.21", generator: "oracle/cm5-clike-editor.mjs", docs, electric }, null, 1) + "\n");
console .log (`${docs .length} documents, ${electric .length} electric cases -> ${OUT}`);
