/**
 * findWordAt with the wordChars helper (issue #57, src/edit/methods.js:335-350, isWordChar in src/util/misc.js:129-133)
 * recorded from CodeMirror 5.65.21 in a headless browser. javascript registers wordChars /[\w$]/; the punctuation
 * branch tests isWordChar(ch) without the helper.
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build CM5_PLAYWRIGHT_FROM=<dir>/ [CM5_BROWSER=chromium|firefox|webkit] \
 *     node oracle/cm5-find-word-at.mjs --out out.json
 */
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { extname, isAbsolute, join, normalize, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

if (!process.env.CM5_BUILD_DIR) throw new Error("set CM5_BUILD_DIR");
const ROOT = normalize(process.env.CM5_BUILD_DIR);
const HERE = normalize(join(fileURLToPath(import.meta.url), ".."));
const pw = createRequire(process.env.CM5_PLAYWRIGHT_FROM || join(HERE, "../../../x_ite-tests/"))("playwright");
const engine = process.env.CM5_BROWSER || "chromium";
const args = process.argv.slice(2);
const OUT = args.includes("--out") ? args[args.indexOf("--out") + 1] : null;
const HARNESS = `<!doctype html><meta charset="utf-8"><link rel="stylesheet" href="/lib/codemirror.css">
<style>body { margin: 0 } .CodeMirror { font-family: "DejaVu Sans Mono"; font-size: 32px; line-height: 38px }</style>`;
const server = createServer(async (req, res) => {
  let rel;
  try { rel = decodeURIComponent(new URL(req.url, "http://x").pathname); } catch { res.writeHead(400); return res.end(); }
  if (rel === "/__harness.html") { res.writeHead(200, { "Content-Type": "text/html" }); return res.end(HARNESS); }
  const path = normalize(join(ROOT, rel)), r = relative(ROOT, path);
  if (r === ".." || r.startsWith(".." + sep) || isAbsolute(r)) { res.writeHead(403); return res.end(); }
  try { const body = await readFile(path); res.writeHead(200, { "Content-Type": { ".js": "text/javascript", ".css": "text/css" }[extname(path)] || "application/octet-stream" }); res.end(body); }
  catch { res.writeHead(404); res.end(); }
});
await new Promise(r => server.listen(0, "127.0.0.1", r));
let browser, out;
try {
browser = await pw[engine].launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1200, height: 900 } });
await page.goto(`http://127.0.0.1:${server.address().port}/__harness.html`);
await page.addScriptTag({ url: "/lib/codemirror.js" });
await page.waitForFunction(() => window.CodeMirror);
await page.addScriptTag({ url: "/mode/javascript/javascript.js" });
await page.evaluate(() => document.fonts.ready);

out = await page.evaluate(() => {
  const results = { version: CodeMirror.version, cases: [] };
  const texts = ["x = +$y", "a$b c", "\u00e1$ z", "foo  bar", "--$$--", "a+$b", "$", "q.$x_1 ", "\u00e9t\u00e9$1", "  \t$a"];
  for (const mode of ["javascript", null]) {
    for (const text of texts) {
      const cm = CodeMirror(document.body, { value: text, mode });
      for (let ch = 0; ch <= text.length; ch++) for (const sticky of [null, "before", "after"]) {
        const r = cm.findWordAt(CodeMirror.Pos(0, ch, sticky));
        results.cases.push({ mode, text, ch, sticky, from: r.anchor.ch, to: r.head.ch });
      }
      cm.getWrapperElement().remove();
    }
  }
  return results;
});
} finally {
  await browser?.close();
  server.close();
}
console.log(out.cases.length + " cases");
if (OUT) await writeFile(OUT, JSON.stringify(out, null, 1) + "\n");
