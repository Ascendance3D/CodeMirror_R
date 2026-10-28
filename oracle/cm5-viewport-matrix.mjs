/**
 * Viewport differential matrix (2026-10-26-p1-final-independent-qa finding M3) for CodeMirror 5.65.21 in headless Chromium.
 * Measurement is normalized to cm_view::Metrics::oracle(): DejaVu Sans Mono 32px,
 * line-height 38px, .CodeMirror-lines padding 4px; editor setSize(600, 300).
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build node cm5-viewport-matrix.mjs --out out.json
 */
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { extname, join, normalize } from "node:path";
import { safeResolve } from "./server-utils.mjs";
import { fileURLToPath } from "node:url";

const ROOT = normalize(process.env.CM5_BUILD_DIR || "");
if (!process.env.CM5_BUILD_DIR) throw new Error("set CM5_BUILD_DIR");
const HERE = normalize(join(fileURLToPath(import.meta.url), ".."));
const { chromium } = createRequire(process.env.CM5_PLAYWRIGHT_FROM || join(HERE, "../../../x_ite-tests/"))("playwright");
const args = process.argv.slice(2);
const OUT = args.includes("--out") ? args[args.indexOf("--out") + 1] : null;
const HARNESS = `<!doctype html><meta charset="utf-8"><link rel="stylesheet" href="/lib/codemirror.css">
<style>body { margin: 0 } .CodeMirror { font-family: "DejaVu Sans Mono"; font-size: 32px; line-height: 38px }</style>`;
const server = createServer(async (req, res) => {
  const url = new URL(req.url, "http://x");
  if (url.pathname === "/__harness.html") { res.writeHead(200, { "Content-Type": "text/html" }); return res.end(HARNESS); }
  const path = normalize(join(ROOT, decodeURIComponent(url.pathname)));
  if (!path.startsWith(ROOT)) { res.writeHead(403); return res.end(); }
  try { const body = await readFile(path); res.writeHead(200, { "Content-Type": { ".js": "text/javascript", ".css": "text/css" }[extname(path)] || "application/octet-stream" }); res.end(body); }
  catch { res.writeHead(404); res.end(); }
});
await new Promise(r => server.listen(0, "127.0.0.1", r));
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1200, height: 900 } });
await page.goto(`http://127.0.0.1:${server.address().port}/__harness.html`);
await page.addScriptTag({ url: "/lib/codemirror.js" });
await page.waitForFunction(() => window.CodeMirror);
await page.evaluate(() => document.fonts.ready);

const out = await page.evaluate(async () => {
  const Pos = CodeMirror.Pos;
  const tick = () => new Promise(r => setTimeout(r, 60));
  const lines = n => Array.from({ length: n }, (_, i) => "line " + i).join("\n");
  const p = c => c.line + ":" + c.ch;
  const state = cm => {
    const s = cm.getScrollInfo(), vp = cm.getViewport();
    const first = cm.lineAtHeight(s.top, "local"), last = cm.lineAtHeight(s.top + s.clientHeight, "local");
    return { vp: [vp.from, vp.to], top: s.top, left: s.left, height: s.height, lines: cm.lineCount(),
             visible: [first, last], firstText: cm.getLine(first) };
  };
  const make = (opts = {}, value = lines(200)) => {
    const cm = CodeMirror(document.body, Object.assign({ value }, opts));
    cm.setSize(600, 300);
    return cm;
  };
  const watch = (cm, log, tag) => {
    cm.on("change", (_, c) => log.push(`${tag}change(${p(c.from)},${p(c.to)},${JSON.stringify(c.text)})`));
    cm.on("changes", (_, cs) => log.push(`${tag}changes(${cs.length})`));
    cm.on("update", () => log.push(`${tag}update`));
    cm.on("viewportChange", (_, a, b) => log.push(`${tag}viewportChange(${a},${b})`));
    cm.on("scroll", () => log.push(`${tag}scroll`));
  };
  const results = { version: CodeMirror.version, cases: {} };
  // One case: a fresh editor scrolled to `top`, then `act` with the log on.
  async function run(name, act, { opts = {}, value, top = 3000, left = null, setup = null } = {}) {
    const cm = make(opts, value);
    if (setup) setup(cm);
    cm.scrollTo(left, top);
    await tick();
    const log = [];
    watch(cm, log, "");
    const before = state(cm);
    await act(cm, log);
    await tick();
    results.cases[name] = { before, after: state(cm), log };
    cm.getWrapperElement().remove();
  }
  const op = (cm, f) => cm.operation(f);
  const rr = (cm, t, a, b) => cm.replaceRange(t, a, b);

  await run("m01_above_insert_line", cm => rr(cm, "X\n", Pos(0, 0)));
  await run("m02_below_insert_line", cm => rr(cm, "X\n", Pos(190, 0)));
  await run("m03_inside_char", cm => rr(cm, "X", Pos(80, 0)));
  await run("m03b_inside_insert_line", cm => rr(cm, "X\n", Pos(80, 0)));
  // Top boundary: a change ending exactly at viewFrom, and one starting at viewFrom.
  await run("m04a_top_boundary_before", cm => { const f = cm.getViewport().from; rr(cm, "X\n", Pos(f - 1, 0)); });
  await run("m04b_top_boundary_first_line", cm => { const f = cm.getViewport().from; rr(cm, "X\n", Pos(f, 0)); });
  await run("m04c_top_boundary_char_first_line", cm => { const f = cm.getViewport().from; rr(cm, "X", Pos(f, 0)); });
  await run("m04d_top_boundary_char_before", cm => { const f = cm.getViewport().from; rr(cm, "X", Pos(f - 1, 0)); });
  // Bottom boundary: last rendered line, and the first line after it.
  await run("m05a_bottom_boundary_last_line", cm => { const t = cm.getViewport().to; rr(cm, "X\n", Pos(t - 1, 0)); });
  await run("m05b_bottom_boundary_after", cm => { const t = cm.getViewport().to; rr(cm, "X\n", Pos(t, 0)); });
  await run("m05c_bottom_boundary_char_after", cm => { const t = cm.getViewport().to; rr(cm, "X", Pos(t, 0)); });
  // Crossing a boundary.
  await run("m06a_cross_top_delete", cm => { const f = cm.getViewport().from; rr(cm, "", Pos(f - 2, 0), Pos(f + 2, 0)); });
  await run("m06b_cross_bottom_delete", cm => { const t = cm.getViewport().to; rr(cm, "", Pos(t - 2, 0), Pos(t + 2, 0)); });
  await run("m06c_cross_whole_view", cm => { const vp = cm.getViewport(); rr(cm, "Z", Pos(vp.from - 1, 0), Pos(vp.to + 1, 0)); });
  // Adding and deleting lines.
  await run("m07a_above_insert_five", cm => rr(cm, "a\nb\nc\nd\ne\n", Pos(3, 0)));
  await run("m07b_inside_insert_five", cm => rr(cm, "a\nb\nc\nd\ne\n", Pos(82, 0)));
  await run("m08a_above_delete_line", cm => rr(cm, "", Pos(0, 0), Pos(1, 0)));
  await run("m08b_above_delete_three", cm => rr(cm, "", Pos(2, 0), Pos(5, 0)));
  await run("m08c_inside_delete_line", cm => rr(cm, "", Pos(81, 0), Pos(82, 0)));
  await run("m08d_below_delete_line", cm => rr(cm, "", Pos(190, 0), Pos(191, 0)));
  // Replacing without a line count change.
  await run("m09a_above_replace_one_line", cm => rr(cm, "QQQQ", Pos(0, 0), Pos(0, 4)));
  await run("m09b_above_replace_multi_same_count", cm => rr(cm, "a\nb\nc", Pos(1, 0), Pos(3, 6)));
  await run("m09c_inside_replace_multi_same_count", cm => rr(cm, "a\nb\nc", Pos(81, 0), Pos(83, 6)));
  // Measured line height: with lineWrapping, a line above that wraps into more rows.
  const long = "w".repeat(70);
  await run("m10a_wrap_above_grows", cm => rr(cm, long, Pos(0, 0)), { opts: { lineWrapping: true } });
  await run("m10b_wrap_above_shrinks", cm => rr(cm, "", Pos(0, 0), Pos(0, 70)),
            { opts: { lineWrapping: true }, setup: cm => cm.replaceRange(long, Pos(0, 0)) });
  // Wrapped geometry inside the view.
  await run("m11a_wrap_inside_grows", cm => { const f = cm.lineAtHeight(cm.getScrollInfo().top, "local"); rr(cm, long, Pos(f + 2, 0)); },
            { opts: { lineWrapping: true } });
  await run("m11b_wrap_below_grows", cm => rr(cm, long, Pos(190, 0)), { opts: { lineWrapping: true } });
  // Horizontally scrolled.
  await run("m12a_hscroll_above_insert_line", cm => rr(cm, "X\n", Pos(0, 0)),
            { left: 200, setup: cm => cm.replaceRange("h".repeat(120), Pos(85, 0)) });
  await run("m12b_hscroll_above_char", cm => rr(cm, "X", Pos(0, 0)),
            { left: 200, setup: cm => cm.replaceRange("h".repeat(120), Pos(85, 0)) });
  // Operations.
  await run("x01_nested_above_and_below", cm => op(cm, () => { op(cm, () => rr(cm, "X\n", Pos(0, 0))); rr(cm, "Y\n", Pos(190, 0)); }));
  await run("x02_multi_insert_then_delete_above", cm => op(cm, () => { rr(cm, "X\n", Pos(0, 0)); rr(cm, "", Pos(0, 0), Pos(1, 0)); }));
  await run("x03_multi_two_above", cm => op(cm, () => { rr(cm, "X\n", Pos(0, 0)); rr(cm, "Y\n", Pos(5, 0)); }));
  await run("x04_direct_doc_above", cm => cm.getDoc().replaceRange("X\n", Pos(0, 0)));
  await run("x05_direct_doc_char_above", cm => cm.getDoc().replaceRange("X", Pos(0, 0)));
  // Scrolling.
  await run("s01_scroll_op_from_top", cm => cm.scrollTo(null, 3000), { top: 0 });
  await run("s02_scroll_op_same", cm => cm.scrollTo(null, 3000));
  await run("s03_scroll_op_small", cm => cm.scrollTo(null, 3000 + 2 * 38));
  await run("s04_native_scroll_far", async cm => { cm.getScrollerElement().scrollTop = 3000; await tick(); }, { top: 0 });
  await run("s05_native_scroll_small", async cm => { cm.getScrollerElement().scrollTop = 3000 + 2 * 38; await tick(); });
  await run("s06_empty_op", cm => op(cm, () => {}));
  // Linked documents: an edit above in one editor of two scrolled linked editors.
  {
    const a = make(), b = make({ value: a.getDoc().linkedDoc({ sharedHist: true }) });
    a.scrollTo(null, 3000); b.scrollTo(null, 3000); await tick();
    const log = []; watch(a, log, "A:"); watch(b, log, "B:");
    const before = { a: state(a), b: state(b) };
    a.replaceRange("X\n", Pos(0, 0)); await tick();
    const mid = { a: state(a), b: state(b) };
    const log2 = log.splice(0);
    b.replaceRange("X", Pos(0, 0)); await tick();
    results.cases.l01_linked_above = { before, after: mid, log: log2, after2: { a: state(a), b: state(b) }, log2: log.splice(0) };
    a.getWrapperElement().remove(); b.getWrapperElement().remove();
  }
  return results;
});
const json = JSON.stringify(out, null, 1);
if (OUT) await writeFile(OUT, json + "\n"); else console.log(json);
await browser.close(); server.close();
