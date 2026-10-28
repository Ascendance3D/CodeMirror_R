/**
 * Wheel recordings with dx != 0 (issue #66) for CodeMirror 5.65.21 in headless Chromium, Firefox and WebKit.
 * onScrollWheel (src/display/scroll_events.js:91) handles a wheel with dx != 0 in JS unless the browser is
 * Gecko: updateScrollTop(max(0, scrollTop + dy)) when dy and the editor can scroll vertically, then
 * setScrollLeft(max(0, scrollLeft + dx)), both subject to the < 2px thresholds. A vertical-only wheel is left
 * to the browser, whose scroll event reaches updateScrollTop / setScrollLeft(.., true).
 * Same page metrics as cm5-scroll-threshold.mjs (setSize(600, 300), editor 200px below the window top).
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build node cm5-wheel-dx.mjs --out out.json
 */
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

if (!process.env.CM5_BUILD_DIR) throw new Error("set CM5_BUILD_DIR");
const ROOT = normalize(process.env.CM5_BUILD_DIR);
const HERE = normalize(join(fileURLToPath(import.meta.url), ".."));
const pw = createRequire(process.env.CM5_PLAYWRIGHT_FROM || join(HERE, "../../../x_ite-tests/"))("playwright");
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

// [name, start left, start top, dx, dy]; 1e9 starts at the maximum, "max-N" N px short of the maximum left.
const CASES = [
  ["v_down_1_mid", 300, 800, 0, 1], ["v_down_100_mid", 300, 800, 0, 100],
  ["h_right_1_mid", 300, 800, 1, 0], ["h_right_2_mid", 300, 800, 2, 0], ["h_right_10_mid", 300, 800, 10, 0],
  ["h_left_1_mid", 300, 800, -1, 0], ["h_left_10_mid", 300, 800, -10, 0],
  ["h_left_500_to_zero", 300, 800, -500, 0], ["h_right_100_at_max", 1e9, 800, 100, 0], ["h_right_1_at_max_minus_1", "max-1", 800, 1, 0],
  ["h_right_1_at_max_minus_5", "max-5", 800, 1, 0], ["h_left_1_at_zero", 0, 800, -1, 0],
  ["d_1_1_mid", 300, 800, 1, 1], ["d_10_1_mid", 300, 800, 10, 1], ["d_1_10_mid", 300, 800, 1, 10], ["d_10_10_mid", 300, 800, 10, 10],
  ["d_1_neg1_mid", 300, 800, 1, -1], ["d_1_neg_big_to_zero", 300, 800, 1, -2000],
  ["d_10_100_at_max_top", 300, 1e9, 10, 100], ["d_10_neg100_at_zero_top", 300, 0, 10, -100],
];

const results = { cases: {}, browsers: {} };
for (const name of ["chromium", "firefox", "webkit"]) {
  const browser = await pw[name].launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1200, height: 900 } });
  await page.goto(`http://127.0.0.1:${server.address().port}/__harness.html`);
  await page.addScriptTag({ url: "/lib/codemirror.js" });
  await page.waitForFunction(() => window.CodeMirror);
  await page.evaluate(() => document.fonts.ready);
  results.version = await page.evaluate(() => CodeMirror.version);
  results.browsers[name] = { gecko: await page.evaluate(() => /gecko\/\d/i.test(navigator.userAgent)), ua: await page.evaluate(() => navigator.userAgent) };
  for (const [cname, left, top, dx, dy] of CASES) {
    const info = await page.evaluate(async ([left, top]) => {
      const doc = Array.from({ length: 60 }, () => "x".repeat(150)).join("\n");
      window.cm = CodeMirror(document.body, { value: doc }); cm.setSize(600, 300);
      const s = cm.getScrollInfo();
      const maxLeft = s.width - s.clientWidth;
      cm.scrollTo(left === "max-1" ? maxLeft - 1 : left === "max-5" ? maxLeft - 5 : left, top);
      await new Promise(r => setTimeout(r, 100));
      return { maxLeft, maxTop: s.height - s.clientHeight };
    }, [left, top]);
    await page.mouse.move(300, 350);
    // Primes wheelPixelsPerUnit like cm5-scroll-threshold.mjs; the editor is then put back.
    for (let i = 0; i < 2; i++) { await page.mouse.wheel(0, 1); await page.waitForTimeout(100); }
    await page.evaluate(async ([left, top]) => {
      const s = cm.getScrollInfo(); const maxLeft = s.width - s.clientWidth;
      cm.scrollTo(left === "max-1" ? maxLeft - 1 : left === "max-5" ? maxLeft - 5 : left, top);
      await new Promise(r => setTimeout(r, 100));
      window.__log = []; cm.on("update", () => __log.push("update")); cm.on("scroll", () => __log.push("scroll"));
      const t = cm.getScrollInfo(); window.__before = { left: t.left, top: t.top };
    }, [left, top]);
    await page.mouse.wheel(dx, dy); await page.waitForTimeout(300);
    const r = await page.evaluate(() => { const t = cm.getScrollInfo(); const r = { before: __before, after: { left: t.left, top: t.top }, log: __log }; cm.getWrapperElement().remove(); return r; });
    (results.cases[cname] ||= { dx, dy, ...info, by: {} }).by[name] = r;
  }
  await browser.close();
}
const text = JSON.stringify(results, null, 1) + "\n";
console.log(text);
if (OUT) await writeFile(OUT, text);
server.close();
