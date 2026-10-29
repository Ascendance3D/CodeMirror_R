/**
 * Gutter markers (issue #36: setGutterMarker / clearGutter / lineInfo, src/model/Doc.js:228-263, isEmpty in
 * src/util/misc.js:135) recorded from CodeMirror 5.65.21 in a headless browser: each step's redraw count ("update"
 * events), the return value and every line's gutterMarkers. A marker is a truthy object here (CM5 stores a DOM node).
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build CM5_PLAYWRIGHT_FROM=<dir>/ [CM5_BROWSER=chromium|firefox|webkit] \
 *     node oracle/cm5-gutter-model.mjs --out out.json
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
await page.evaluate(() => document.fonts.ready);

out = await page.evaluate(() => {
  const results = { version: CodeMirror.version, cases: {} };
  const val = v => v === "N" ? {} : v === "null" ? null : v === "undef" ? undefined : v === "zero" ? 0 : v === "empty" ? "" : v;
  // A marker map as [id, "M" | null] pairs in key order, or null.
  const shape = m => m ? Object.keys(m).map(k => [k, m[k] ? "M" : null]) : null;
  const run = (name, steps, doc = "zero\none\ntwo\nthree") => {
    const cm = CodeMirror(document.body, { value: doc });
    cm.setSize(600, 300);
    let updates = 0;
    cm.on("update", () => updates++);
    const handles = [0, 1, 2, 3].map(n => cm.getLineHandle(n));
    const arg = l => (l && typeof l === "object") ? handles[l.h] : l;
    const recs = [];
    for (const [op, a, b, c] of steps) {
      const before = updates;
      let ret = null, info, err = null;
      try {
      if (op === "set") { const r = cm.setGutterMarker(arg(a), b, val(c)); ret = r ? "line" : String(r); }
      else if (op === "clear") cm.clearGutter(a);
      else if (op === "info") {
        const i = cm.lineInfo(arg(a));
        info = i === null ? null : { line: i.line, text: i.text, markers: shape(i.gutterMarkers), keys: Object.keys(i).sort(), sameHandle: i.handle === cm.getLineHandle(i.line) };
      }
      else if (op === "edit") {
        if (a === "delete_line1") cm.replaceRange("", { line: 0, ch: 4 }, { line: 1, ch: 3 });
        else if (a === "append") cm.replaceRange("x", { line: 1, ch: 0 });
        else if (a === "set_value") cm.setValue("a\nb\nc\nd");
      }
      } catch (e) { err = e.name; }
      recs.push({ step: [op, a, b, c], ret, err, redraw: updates - before, ...(info !== undefined ? { info } : {}), lines: [0, 1, 2, 3].map(n => cm.lineCount() > n ? shape(cm.getLineHandle(n).gutterMarkers) : "none") });
    }
    results.cases[name] = recs;
    cm.getWrapperElement().remove();
  };
  run("set_then_clear_last", [["set", 0, "A", "N"], ["set", 0, "A", "null"]]);
  run("stale_null_key_kept", [["set", 0, "A", "N"], ["set", 0, "B", "N"], ["set", 0, "A", "null"], ["info", 0]]);
  run("stale_null_key_dropped", [["set", 0, "A", "N"], ["set", 0, "B", "N"], ["set", 0, "A", "null"], ["set", 0, "B", "null"]]);
  run("falsy_on_empty_line", [["set", 0, "A", "null"], ["set", 0, "A", "zero"], ["set", 0, "A", "empty"], ["set", 0, "A", "undef"]]);
  run("falsy_values_keep_entry", [["set", 0, "A", "N"], ["set", 0, "B", "zero"], ["set", 0, "C", "empty"], ["set", 0, "A", "null"]]);
  run("same_marker_twice_redraws", [["set", 0, "A", "N"], ["set", 0, "A", "N"]]);
  run("clear_gutter_all_lines", [["set", 0, "A", "N"], ["set", 2, "A", "N"], ["set", 1, "B", "N"], ["set", 3, "A", "null"], ["clear", "A"]]);
  run("clear_gutter_keeps_other_ids", [["set", 0, "A", "N"], ["set", 0, "B", "N"], ["set", 1, "A", "N"], ["clear", "A"], ["clear", "B"], ["clear", "B"]]);
  run("clear_gutter_unknown_id", [["set", 0, "A", "N"], ["clear", "Z"]]);
  run("clear_gutter_falsy_entry_skipped", [["set", 0, "A", "N"], ["set", 0, "B", "N"], ["set", 0, "B", "null"], ["clear", "B"]]);
  run("number_clipping", [["set", 99, "A", "N"], ["set", -5, "B", "N"], ["set", 1.5, "C", "N"]]);
  run("line_info_numbers", [["info", -1], ["info", 0], ["info", 3], ["info", 4], ["info", 99], ["set", 2, "A", "N"], ["info", 2]]);
  run("line_info_after_delete", [["set", { h: 1 }, "A", "N"], ["info", { h: 1 }], ["edit", "delete_line1"], ["info", { h: 1 }], ["set", { h: 1 }, "B", "N"], ["info", 1]]);
  run("markers_survive_text_edit", [["set", 1, "A", "N"], ["edit", "append"], ["info", 1]]);
  run("markers_lost_on_set_value", [["set", 1, "A", "N"], ["edit", "set_value"], ["info", 1]]);
  run("handle_arg_set_and_clear", [["set", { h: 2 }, "A", "N"], ["set", { h: 2 }, "A", "null"]]);
  return results;
});
} finally {
  await browser?.close();
  server.close();
}
console.log(JSON.stringify(out, null, 1).slice(0, 300));
if (OUT) await writeFile(OUT, JSON.stringify(out, null, 1) + "\n");
