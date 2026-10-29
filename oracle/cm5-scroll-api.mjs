/**
 * Scroll API (issue #54: scrollTo, getScrollInfo and scrollIntoView, src/edit/methods.js:364-390; the rect form
 * scrollToCoordsRange and the rect helper scrollIntoView, src/display/scrolling.js:70,149) recorded from
 * CodeMirror 5.65.21 in headless Chromium. Same page metrics as cm5-viewport-matrix.mjs
 * (cm_view::Metrics::oracle(): DejaVu Sans Mono 32px, line-height 38px, padding 4px; setSize(600, 300)).
 * The editor sits 200 px below the window top, so maybeScrollWindow stays out of the way.
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build CM5_PLAYWRIGHT_FROM=<dir>/ \
 *     node oracle/cm5-scroll-api.mjs --out crates/cm-view/tests/data/cm5-scroll-api.json
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
const args = process.argv.slice(2);
const OUT = args.includes("--out") ? args[args.indexOf("--out") + 1] : null;
const HARNESS = `<!doctype html><meta charset="utf-8"><link rel="stylesheet" href="/lib/codemirror.css">
<style>body { margin: 0; padding-top: 200px } .CodeMirror { font-family: "DejaVu Sans Mono"; font-size: 32px; line-height: 38px }
.CodeMirror-scroll { overflow-anchor: none }</style>`;
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
  browser = await pw.chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1200, height: 900 } });
  await page.goto(`http://127.0.0.1:${server.address().port}/__harness.html`);
  await page.addScriptTag({ url: "/lib/codemirror.js" });
  await page.waitForFunction(() => window.CodeMirror);
  await page.evaluate(() => document.fonts.ready);
  out = await page.evaluate(async () => {
    const Pos = CodeMirror.Pos;
    const tick = () => new Promise(r => setTimeout(r, 80));
    const results = { version: CodeMirror.version, info: {}, cases: {} };
    const docs = {
      long: Array.from({ length: 60 }, (_, i) => i == 30 ? "y".repeat(150) : `line ${i}`).join("\n"),
      short: "a\nbb\nccc",
      wide: Array.from({ length: 5 }, () => "x".repeat(150)).join("\n"),
    };
    const info = cm => { const s = cm.getScrollInfo(); return { left: s.left, top: s.top, height: s.height, width: s.width, clientHeight: s.clientHeight, clientWidth: s.clientWidth }; };
    // getScrollInfo for several documents and options.
    for (const [name, doc, opts, scroll] of [["long", "long", {}, [0, 0]], ["long_scrolled", "long", {}, [100, 500]], ["short", "short", {}, [0, 0]],
      ["wide", "wide", {}, [0, 0]], ["wide_max", "wide", {}, [1e9, 1e9]], ["long_numbers", "long", { lineNumbers: true }, [50, 700]],
      ["wrapped", "wide", { lineWrapping: true }, [0, 40]], ["long_max", "long", {}, [1e9, 1e9]]]) {
      const cm = CodeMirror(document.body, Object.assign({ value: docs[doc] }, opts)); cm.setSize(600, 300); await tick();
      cm.scrollTo(scroll[0], scroll[1]); await tick();
      results.info[name] = info(cm);
      cm.getWrapperElement().remove();
    }
    // scrollIntoView forms. Each case: a fresh editor ("long", cursor at (40, 3)), a setup scroll, then one call.
    async function run(name, setup, act, opts = {}) {
      const cm = CodeMirror(document.body, Object.assign({ value: docs.long }, opts)); cm.setSize(600, 300);
      cm.setCursor(Pos(40, 3), null, { scroll: false }); await tick();
      cm.scrollTo(setup[0], setup[1]); await tick();
      const before = info(cm);
      act(cm); await tick();
      results.cases[name] = { setup, before, after: info(cm) };
      cm.getWrapperElement().remove();
    }
    await run("null", [0, 0], cm => cm.scrollIntoView(null));
    await run("null_margin_option", [0, 0], cm => cm.scrollIntoView(null), { cursorScrollMargin: 50 });
    await run("null_margin_given", [0, 0], cm => cm.scrollIntoView(null, 20), { cursorScrollMargin: 50 });
    await run("null_margin_zero_given", [0, 0], cm => cm.scrollIntoView(null, 0), { cursorScrollMargin: 50 });
    await run("line_number", [0, 0], cm => cm.scrollIntoView(50));
    await run("line_number_margin", [0, 0], cm => cm.scrollIntoView(50, 30));
    await run("line_number_up", [0, 1500], cm => cm.scrollIntoView(5));
    await run("pos", [0, 0], cm => cm.scrollIntoView(Pos(30, 140)));
    await run("pos_margin", [0, 1500], cm => cm.scrollIntoView(Pos(20, 0), 40));
    await run("range", [0, 0], cm => cm.scrollIntoView({ from: Pos(30, 2), to: Pos(45, 0) }));
    await run("range_from_only", [0, 0], cm => cm.scrollIntoView({ from: Pos(50, 0) }));
    await run("coords", [0, 0], cm => cm.scrollIntoView({ left: 100, top: 1200, right: 120, bottom: 1240 }));
    await run("coords_margin", [0, 0], cm => cm.scrollIntoView({ left: 100, top: 1200, right: 120, bottom: 1240 }, 25));
    await run("coords_wide", [0, 0], cm => cm.scrollIntoView({ left: 2000, top: 1200, right: 2100, bottom: 1240 }));
    await run("coords_up", [300, 1500], cm => cm.scrollIntoView({ left: 50, top: 300, right: 60, bottom: 330 }));
    await run("coords_range", [0, 0], cm => cm.scrollIntoView({ from: { left: 100, top: 1200, right: 120, bottom: 1240 }, to: { left: 500, top: 1400, right: 520, bottom: 1450 } }));
    await run("coords_range_margin", [0, 0], cm => cm.scrollIntoView({ from: { left: 100, top: 1200, right: 120, bottom: 1240 }, to: { left: 500, top: 1400, right: 520, bottom: 1450 } }, 10));
    await run("coords_top_snap", [0, 1000], cm => cm.scrollIntoView({ left: 0, top: 20, right: 0, bottom: 50 }));
    await run("coords_bottom_snap", [0, 0], cm => cm.scrollIntoView({ left: 0, top: 2200, right: 0, bottom: 2290 }));
    await run("coords_tall", [0, 0], cm => cm.scrollIntoView({ left: 0, top: 1000, right: 0, bottom: 1900 }));
    // scrollTo in one operation with other scroll requests.
    await run("op_into_then_to", [0, 0], cm => cm.operation(() => { cm.scrollIntoView(Pos(50, 0)); cm.scrollTo(null, 100); }));
    await run("op_to_then_into", [0, 0], cm => cm.operation(() => { cm.scrollTo(null, 100); cm.scrollIntoView(Pos(50, 0)); }));
    await run("op_cursor_then_to", [0, 0], cm => cm.operation(() => { cm.setCursor(Pos(50, 0)); cm.scrollTo(null, 100); }));
    await run("op_to_x_only", [0, 0], cm => cm.operation(() => { cm.scrollIntoView(Pos(50, 0)); cm.scrollTo(200, null); }));
    await run("op_to_null_null", [0, 0], cm => cm.operation(() => { cm.scrollIntoView(Pos(50, 0)); cm.scrollTo(null, null); }));
    await run("op_coords_then_to", [0, 0], cm => cm.operation(() => { cm.scrollIntoView({ left: 0, top: 1200, right: 0, bottom: 1240 }); cm.scrollTo(null, 100); }));
    return results;
  });
} finally {
  if (browser) await browser.close();
  server.close();
}
const json = JSON.stringify(out, null, 1) + "\n";
if (OUT) await writeFile(OUT, json); else process.stdout.write(json);
