/**
 * Scroll-threshold (issue #66) boundary recordings for CodeMirror 5.65.21 in headless Chromium.
 * Same page metrics as cm5-viewport-matrix.mjs (cm_view::Metrics::oracle(): DejaVu
 * Sans Mono 32px, line-height 38px, padding 4px; setSize(600, 300)).
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build node cm5-scroll-threshold.mjs --out out.json
 */
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = normalize(process.env.CM5_BUILD_DIR || "");
if (!process.env.CM5_BUILD_DIR) throw new Error("set CM5_BUILD_DIR");
const HERE = normalize(join(fileURLToPath(import.meta.url), ".."));
const { chromium } = createRequire(process.env.CM5_PLAYWRIGHT_FROM || join(HERE, "../../../x_ite-tests/"))("playwright");
const args = process.argv.slice(2);
const OUT = args.includes("--out") ? args[args.indexOf("--out") + 1] : null;
const HARNESS = `<!doctype html><meta charset="utf-8"><link rel="stylesheet" href="/lib/codemirror.css">
<style>body { margin: 0; padding-top: 200px } .CodeMirror { font-family: "DejaVu Sans Mono"; font-size: 32px; line-height: 38px } .CodeMirror-scroll { overflow-anchor: none }</style>`;
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


// Issue #66 boundary recordings: scrollPosIntoView (scrollIntoView with a Pos) reaches the
// thresholded updateScrollTop / setScrollLeft; scrollTo and the scroller's own scroll event do not.
// The editor sits 200 px below the window top, so maybeScrollWindow stays out of the way (window_top_* cases use 0).
// Doc "long" = 60 lines of 150 "x" (as crates/cm-view/tests/scroll_clamp.rs); 600x300, no wrapping.
const out = await page.evaluate(async () => {
  const Pos = CodeMirror.Pos;
  const tick = () => new Promise(r => setTimeout(r, 80));
  const doc = Array.from({ length: 60 }, () => "x".repeat(150)).join("\n");
  const results = { version: CodeMirror.version, cases: {} };
  const make = () => { const cm = CodeMirror(document.body, { value: doc }); cm.setSize(600, 300); return cm; };
  const st = cm => { const s = cm.getScrollInfo(); const vp = cm.getViewport();
    return { top: s.top, left: s.left, maxTop: s.height - s.clientHeight, maxLeft: s.width - s.clientWidth, vp: [vp.from, vp.to] }; };
  async function run(name, setup, act, atWindowTop = false) {
    document.body.style.paddingTop = atWindowTop ? "0" : "200px";
    const cm = make(); await tick();
    await setup(cm); await tick();
    const log = [];
    cm.on("scroll", () => log.push("scroll")); cm.on("update", () => log.push("update"));
    cm.on("viewportChange", (_, a, b) => log.push(`viewportChange(${a},${b})`));
    await tick(); window.__trace = []; const before = st(cm);
    await act(cm); await tick();
    results.cases[name] = { before, after: st(cm), log, trace: (window.__trace || []).map(t => t.join(":")) };
    cm.getWrapperElement().remove();
  }
  const native = async (cm, y, x) => { if (y != null) cm.getScrollerElement().scrollTop = y; if (x != null) cm.getScrollerElement().scrollLeft = x; await tick(); };
  const into = (cm, p) => cm.scrollIntoView(p, 0);
  const topOf = (cm, l) => cm.charCoords(Pos(l, 0), "local").top;
  const botOf = (cm, l) => cm.charCoords(Pos(l, 0), "local").bottom;

  // Vertical, upward: the cursor line's top is d px above the view's top.
  for (const d of [0.5, 1, 1.9, 2, 3, 7.5])
    await run(`up_${d}`, cm => cm.scrollTo(null, topOf(cm, 30) + d), cm => into(cm, Pos(30, 0)));
  // Vertical, downward: the line's bottom is d px below the view's bottom.
  for (const d of [1, 1.9, 2, 3])
    await run(`down_${d}`, cm => cm.scrollTo(null, botOf(cm, 30) - 300 - d), cm => into(cm, Pos(30, 0)));
  // The window-level path: with the editor flush against the window top, maybeScrollWindow
  // (scrolling.js:16) calls DOM scrollIntoView, which moves the scroller exactly; the engine has no window.
  for (const d of [1, 1.9])
    await run(`window_top_up_${d}`, cm => cm.scrollTo(null, topOf(cm, 30) + d), cm => into(cm, Pos(30, 0)), true);
  // Vertical, at the top: scrolled to d px, cursor at line 0 (target 0, current d).
  for (const d of [1, 2, 3])
    await run(`to_top_from_${d}`, cm => cm.scrollTo(null, d), cm => into(cm, Pos(0, 0)));
  // Vertical, target beyond the maximum: cursor on the last line, scrolled to max - d.
  for (const d of [0, 1, 1.9, 2, 5])
    await run(`last_line_from_max_minus_${d}`, cm => cm.scrollTo(null, 1e9), async cm => { const m = st(cm).maxTop; cm.scrollTo(null, m - d); await tick(); into(cm, Pos(59, 150)); });
  // Horizontal: end of a 150-char line, scrolled to max - d (target beyond the maximum), and from far.
  for (const d of [0, 1, 1.9, 2, 5])
    await run(`right_end_from_max_minus_${d}`, cm => cm.scrollTo(1e9, null), async cm => { const m = st(cm).maxLeft; cm.scrollTo(m - d, null); await tick(); into(cm, Pos(0, 150)); });
  await run("right_end_from_zero", () => {}, cm => into(cm, Pos(0, 150)));
  // Horizontal, back to column 0: scrollLeft d -> target 0.
  for (const d of [1, 1.9, 2, 3])
    await run(`left_start_from_${d}`, cm => cm.scrollTo(d, null), cm => into(cm, Pos(0, 0)));
  // Repeated small requests: three scrollIntoView calls each 1 px further.
  await run("repeat_up_1px", cm => cm.scrollTo(null, topOf(cm, 30) + 1), async cm => { for (let i = 0; i < 3; i++) { into(cm, Pos(30, 0)); await tick(); } });
  // Explicit scrollTo: every size applies; negative / beyond-max clamp.
  for (const [n, x, y] of [["to_1px", 300, 801], ["to_half", 300, 800.5], ["to_negative", -50, -50], ["to_beyond", 1e9, 1e9], ["to_same", 300, 800]])
    await run(`scrollTo_${n}`, cm => cm.scrollTo(300, 800), cm => cm.scrollTo(x, y));
  await run("scrollTo_negative_at_zero", () => {}, cm => cm.scrollTo(-2, -2));
  await run("scrollTo_beyond_at_max", cm => cm.scrollTo(1e9, 1e9), async cm => { const s = st(cm); cm.scrollTo(s.maxLeft + 2, s.maxTop + 2); });
  // Scroller (native / wheel) movement: applies at every size.
  for (const d of [0.5, 1, 1.9, 2])
    await run(`native_top_${d}`, cm => cm.scrollTo(null, 800), cm => native(cm, 800 + d, null));
  await run("native_left_1", cm => cm.scrollTo(300, null), cm => native(cm, null, 301));
  await run("native_negative", cm => cm.scrollTo(300, 800), cm => native(cm, -5, -5));
  return results;
});

// Wheel (real input): only with dx != 0 does the handler call updateScrollTop(cm, max(0, scrollTop + dy)) with the
// RAW target (scroll_events.js:91); a vertical-only wheel scrolls natively and reaches updateScrollTop through the
// scroller scroll event. At a boundary the clamped change is 0 but the raw one is not. The first wheel events calibrate
// wheelPixelsPerUnit, so each case primes with two 1-unit wheels first.
for (const [name, to, dy, dx] of [["wheel_down_100_at_max", 1e9, 100, 0], ["wheel_up_100_at_zero", 0, -100, 0], ["wheel_down_100_mid", 800, 100, 0], ["wheel_down_1_mid", 800, 1, 0],
  ["wheel_diag_down_100_at_max", 1e9, 100, 10], ["wheel_diag_up_100_at_zero", 0, -100, 10], ["wheel_diag_down_100_mid", 800, 100, 10], ["wheel_diag_down_1_mid", 800, 1, 10], ["wheel_diag_down_1_at_max", 1e9, 1, 10]]) {
  await page.evaluate(async ([to]) => {
    document.body.style.paddingTop = "200px";
    const doc = Array.from({ length: 60 }, () => "x".repeat(150)).join("\n");
    window.cm = CodeMirror(document.body, { value: doc }); cm.setSize(600, 300); cm.scrollTo(null, to);
    await new Promise(r => setTimeout(r, 100));
  }, [to]);
  await page.mouse.move(300, 350);
  for (let i = 0; i < 2; i++) { await page.mouse.wheel(0, 1); await page.waitForTimeout(100); }
  await page.evaluate(([to]) => { cm.scrollTo(null, to); window.__trace = []; window.__log = []; cm.on("update", () => __log.push("update")); cm.on("viewportChange", (_, a, b) => __log.push(`viewportChange(${a},${b})`)); cm.on("scroll", () => __log.push("scroll")); window.__before = cm.getScrollInfo().top; }, [to]);
  await page.waitForTimeout(100);
  await page.evaluate(() => { window.__before = cm.getScrollInfo().top; window.__trace = []; window.__log = []; });
  await page.mouse.wheel(dx, dy); await page.waitForTimeout(300);
  out.cases[name] = await page.evaluate(() => { const r = { before: { top: __before }, after: { top: cm.getScrollInfo().top }, log: __log, trace: (window.__trace || []).map(t => t.join(":")) }; cm.getWrapperElement().remove(); return r; });
}
console.log(JSON.stringify(Object.fromEntries(Object.entries(out.cases).filter(([k]) => k.startsWith("wheel")))));
if (OUT) await writeFile(OUT, JSON.stringify(out, null, 1) + "\n");
await browser.close();
server.close();
