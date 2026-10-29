/**
 * Coordinate systems (issue #53: cursorCoords, charCoords, coordsChar, lineAtHeight, heightAtLine,
 * src/edit/methods.js:184-216; intoCoordSystem / fromCoordSystem, src/measurement/position_measurement.js:308,330)
 * recorded from CodeMirror 5.65.21 in headless Chromium. Same page metrics as cm5-viewport-matrix.mjs
 * (cm_view::Metrics::oracle(): DejaVu Sans Mono 32px, line-height 38px, padding 4px; setSize(600, 300)).
 * The editor sits 200 px below the body top and the page is scrolled, so "page" and "window" differ.
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build CM5_PLAYWRIGHT_FROM=<dir>/ \
 *     node oracle/cm5-coord-systems.mjs --out crates/cm-view/tests/data/cm5-coord-systems.json
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
<style>body { margin: 0; padding-top: 200px; padding-left: 30px } .CodeMirror { font-family: "DejaVu Sans Mono"; font-size: 32px; line-height: 38px }
.CodeMirror-scroll { overflow-anchor: none } #spacer { height: 3000px; width: 3000px }</style>`;
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
    const MODES = [null, "page", "window", "local", "div", "line"];
    const name = m => m == null ? "default" : m;
    const rect = r => ({ left: r.left, right: r.right, top: r.top, bottom: r.bottom });
    const pinfo = p => ({ line: p.line, ch: p.ch, sticky: p.sticky == null ? null : p.sticky, outside: p.outside ? (p.outside > 0 ? 1 : -1) : 0, xRel: p.xRel });
    const results = { version: CodeMirror.version, cases: {} };
    const lines = n => Array.from({ length: n }, (_, i) => i == 85 ? "h".repeat(120) : `line ${i}`).join("\n");
    async function run(cname, opts, scroll, pageScroll) {
      const spacer = document.createElement("div"); spacer.id = "spacer";
      const cm = CodeMirror(document.body, Object.assign({ value: lines(200) }, opts));
      document.body.appendChild(spacer);
      cm.setSize(600, 300);
      cm.setSelection(Pos(85, 3), Pos(82, 1), { scroll: false });
      await tick();
      cm.scrollTo(scroll[0], scroll[1]);
      window.scrollTo(pageScroll[0], pageScroll[1]);
      await tick();
      const w = cm.getWrapperElement().getBoundingClientRect();
      const si = cm.getScrollInfo(), vp = cm.getViewport();
      const c = {
        env: { wrapper: [w.left, w.top], pageScroll: [window.pageXOffset, window.pageYOffset], scroll: [si.left, si.top],
          viewport: [vp.from, vp.to], viewOffset: cm.display.viewOffset, gutters: cm.display.gutters.offsetWidth,
          docHeight: cm.doc.height },
        cursorCoords: [], charCoords: [], coordsChar: [], lineAtHeight: [], heightAtLine: [],
      };
      for (const start of [null, true, false, Pos(84, 2), Pos(500, 3), Pos(-3, 0), Pos(84, 999), Pos(85, 100)])
        for (const m of MODES) c.cursorCoords.push({ start, mode: name(m), r: rect(cm.cursorCoords(start, m)) });
      for (const pos of [Pos(84, 2), Pos(500, 3), Pos(-3, 0), Pos(85, 50), Pos(84, 999)])
        for (const m of MODES) c.charCoords.push({ pos, mode: name(m), r: rect(cm.charCoords(pos, m)) });
      for (const [left, top] of [[100, 300], [-20, -50], [5000, 99999], [200, 3200], [700, 3050], [30, 100]])
        for (const m of MODES.slice(0, 5)) c.coordsChar.push({ left, top, mode: name(m), p: pinfo(cm.coordsChar({ left, top }, m)) });
      for (const h of [-100, 0, 100, 300, 3000, 3100, 7599, 7600, 7700, 1e6])
        for (const m of MODES.slice(0, 5)) c.lineAtHeight.push({ h, mode: name(m), line: cm.lineAtHeight(h, m) });
      for (const line of [-5, 0, 10, 81, 199, 200, 500])
        for (const m of MODES) for (const iw of [false, true])
          c.heightAtLine.push({ line, mode: name(m), includeWidgets: iw, h: cm.heightAtLine(line, m, iw) });
      for (const line of [0, 81, 199])
        for (const m of MODES) c.heightAtLine.push({ handle: line, mode: name(m), includeWidgets: false, h: cm.heightAtLine(cm.getLineHandle(line), m) });
      results.cases[cname] = c;
      cm.getWrapperElement().remove(); spacer.remove();
      window.scrollTo(0, 0);
      await tick();
    }
    await run("scrolled", {}, [40, 3000], [10, 120]);
    await run("line_numbers", { lineNumbers: true }, [40, 3000], [0, 150]);
    await run("top", {}, [0, 0], [0, 0]);
    await run("first_line_5", { lineNumbers: true, firstLineNumber: 5 }, [0, 1500], [0, 50]);
    return results;
  });
} finally {
  if (browser) await browser.close();
  server.close();
}
const json = JSON.stringify(out, null, 1) + "\n";
if (OUT) await writeFile(OUT, json); else process.stdout.write(json);
