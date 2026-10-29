/**
 * runMode options (issue #60: src/line/highlight.js:208-245) recorded from CodeMirror 5.65.21 in a headless
 * browser: the style arrays (`line.styles` without the leading modeGen, and `line.styleClasses`) that
 * getLineStyles computes for each line under `addModeClass` (inner mode name as an `m-<name>` prefix) and the
 * per-mode `flattenSpans` override (`mode.flattenSpans`, here set with CodeMirror.extendMode, beats the option).
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build CM5_PLAYWRIGHT_FROM=<dir>/ [CM5_BROWSER=chromium|firefox|webkit] \
 *     node oracle/cm5-mode-class.mjs --out out.json
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
const HARNESS = `<!doctype html><meta charset="utf-8"><link rel="stylesheet" href="/lib/codemirror.css">`;
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
for (const m of ["xml", "javascript", "css", "htmlmixed"]) await page.addScriptTag({ url: `/mode/${m}/${m}.js` });

out = await page.evaluate(() => {
  const results = { version: CodeMirror.version, cases: {} };
  const run = (name, options, text) => {
    const cm = CodeMirror(document.body, Object.assign({ value: text }, options));
    const lines = [];
    for (let n = 0; n < cm.lineCount(); n++) {
      cm.getTokenTypeAt({ line: n, ch: 0 });
      const h = cm.getLineHandle(n);
      lines.push({ styles: h.styles.slice(1), classes: h.styleClasses ? { bgClass: h.styleClasses.bgClass || null, textClass: h.styleClasses.textClass || null } : null });
    }
    results.cases[name] = { options, text, lines };
    cm.getWrapperElement().remove();
  };
  const js = "var foo = bar(1, 'x');\n// note\nfoo();";
  const html = "<div class=\"a\"></div>\n<script>var x = 1;</script>\n<style>p { color: red }</style>\n\n<b>t</b>";
  run("js_plain", { mode: "javascript" }, js);
  run("js_add_mode_class", { mode: "javascript", addModeClass: true }, js);
  run("html_add_mode_class", { mode: "htmlmixed", addModeClass: true }, html);
  run("html_add_mode_class_unflattened", { mode: "htmlmixed", addModeClass: true, flattenSpans: false }, html);
  run("xml_flatten_option_off", { mode: "xml", flattenSpans: false }, "<a></a><b/>");
  run("xml_flatten_option_on", { mode: "xml" }, "<a></a><b/>");
  // Per-mode flattenSpans (highlight.js:209-210): the mode property beats the option.
  CodeMirror.extendMode("css", { flattenSpans: false });
  CodeMirror.extendMode("xml", { flattenSpans: true });
  run("css_mode_flatten_false", { mode: "css" }, "a{color:red}\nb  {  }");
  run("xml_mode_flatten_true_option_off", { mode: "xml", flattenSpans: false }, "<a></a><b/>");
  run("css_mode_flatten_false_add_mode_class", { mode: "css", addModeClass: true }, "a{color:red}");
  return results;
});
} finally {
  await browser?.close();
  server.close();
}
console.log(JSON.stringify(out).slice(0, 300));
if (OUT) await writeFile(OUT, JSON.stringify(out, null, 1) + "\n");
