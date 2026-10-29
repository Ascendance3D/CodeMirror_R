/**
 * Highlight worker recording (issue #66, startWorker / highlightWorker) for CodeMirror 5.65.21 in headless
 * Chromium. highlightWorker restyles the visible lines it finds different from the on-demand styles and
 * reports them with regLineChange inside an operation, which fires "update" (src/display/highlight_worker.js:47-52).
 * Measurement is normalized to cm_view::Metrics::oracle(): DejaVu Sans Mono 32px, line-height 38px; setSize(600, 300).
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build CM5_PLAYWRIGHT_FROM=<dir>/ node cm5-highlight-worker.mjs --out out.json
 */
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { extname, isAbsolute, join, normalize, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

if (!process.env.CM5_BUILD_DIR) throw new Error("set CM5_BUILD_DIR");
const ROOT = normalize(process.env.CM5_BUILD_DIR);
const HERE = normalize(join(fileURLToPath(import.meta.url), ".."));
const { chromium } = createRequire(process.env.CM5_PLAYWRIGHT_FROM || join(HERE, "../../../x_ite-tests/"))("playwright");
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
let browser;
let out;
try {
browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1200, height: 900 } });
await page.goto(`http://127.0.0.1:${server.address().port}/__harness.html`);
await page.addScriptTag({ url: "/lib/codemirror.js" });
await page.addScriptTag({ url: "/mode/javascript/javascript.js" });
await page.waitForFunction(() => window.CodeMirror);
await page.evaluate(() => document.fonts.ready);

out = await page.evaluate(async () => {
  const wait = ms => new Promise(r => setTimeout(r, ms));
  const results = { version: CodeMirror.version, cases: {} };
  // The visible line's style spans as [length-pairs]: line.styles is [from?, to, style, ...] with the leading mode info.
  const spans = (cm, n) => { const s = cm.getLineHandle(n).styles; return s ? s.slice(1).filter((_, i) => i % 2 == 0).length : null; };
  async function run(name, value, line) {
    const cm = CodeMirror(document.body, { value, mode: "javascript" });
    cm.setSize(600, 300);
    await wait(50);
    const updates = [];
    cm.on("update", () => updates.push(cm.doc.highlightFrontier));
    cm.scrollTo(null, line * 38);
    const vp = cm.getViewport(), vis = vp.from + 3;
    const synchronous = { updates: updates.length, spans: spans(cm, vis) };
    // Wait for the worker to reach the end of the viewport (it runs in slices, workTime 100 / workDelay 100).
    const t0 = Date.now();
    while (cm.doc.highlightFrontier < cm.getViewport().to && Date.now() - t0 < 20000) await wait(20);
    await wait(300);
    results.cases[name] = { line, viewport: [vp.from, vp.to], vis, synchronous, final: { updates: updates.length, spans: spans(cm, vis), frontier_reached: cm.doc.highlightFrontier >= cm.getViewport().to } };
    cm.getWrapperElement().remove();
  }
  const body = n => Array.from({ length: n }, () => "var x = 1;").join("\n");
  await run("w01_comment_started_above_viewport", "/*\n" + body(3000) + "\n*/\nvar y;", 2000);
  await run("w02_no_style_difference", body(3000), 2000);
  return results;
});
} finally {
  await browser?.close();
  server.close();
}
console.log(JSON.stringify(out, null, 1));
if (OUT) await writeFile(OUT, JSON.stringify(out, null, 1) + "\n");
