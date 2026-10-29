/**
 * Line widgets (issue #29: src/model/line_widget.js, widgetHeight src/measurement/widgets.js:4, widgetTopHeight and
 * intoCoordSystem src/measurement/position_measurement.js:297-330, coordsCharInner :463) recorded from CodeMirror
 * 5.65.21 in a headless browser. Each widget node is a div whose CSS height makes the widget box a whole number of px. After every step: the document
 * height, scrollTop, the line tops (heightAtLine local, with and without widgets), cursor coordinates, coordsChar at
 * cursor points, lineAtHeight, lineInfo widget counts, the lineWidget* events and the update count. The page turns
 * off the browser's scroll anchoring (overflow-anchor), which otherwise moves the scroller when a node's style
 * changes, at a time that depends on event order (420 or 445 for the same step).
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build CM5_PLAYWRIGHT_FROM=<dir>/ [CM5_BROWSER=chromium|firefox|webkit] \
 *     node oracle/cm5-line-widgets.mjs --out out.json
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
<style>body { margin: 0 } .CodeMirror { font-family: "DejaVu Sans Mono"; font-size: 32px; line-height: 38px } .g16 { width: 16px } .g30 { width: 30px } .gw { width: 7px } .CodeMirror-scroll { overflow-anchor: none }</style>`;
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

out = await page.evaluate(async () => {
  const results = { version: CodeMirror.version, cases: [] };
  const doc = Array.from({ length: 40 }, (_, i) => "line " + i + " text").join("\n");
  // .CodeMirror-linewidget { padding: 0.1px } adds 2 x 0.09375 px (1/64 px units in Blink and WebKit), so a node of
  // h - 0.1875 px makes the widget box exactly h px, and its offsetHeight (widget.height) h as well.
  const node = h => { const d = document.createElement("div"); d.style.height = (h - 0.1875) + "px"; d.textContent = "w"; return d; };
  const run = async (name, steps, opts = {}) => {
    const cm = CodeMirror(document.body, Object.assign({ value: doc }, opts));
    cm.setSize(600, 300);
    const log = [];
    let updates = 0;
    cm.on("update", () => updates++);
    for (const ev of ["lineWidgetAdded", "lineWidgetCleared", "lineWidgetChanged"])
      cm.on(ev, (_, w, line) => log.push([ev, widgets.indexOf(w), line]));
    const widgets = [];
    const recs = [];
    for (const st of steps) {
      log.length = 0;
      const before = updates;
      const [op, a, b, c] = st;
      if (op === "add") widgets.push(cm.addLineWidget(a, node(b), c || {}));
      else if (op === "clear") widgets[a].clear();
      else if (op === "change") { widgets[a].node.style.height = (b - 0.1875) + "px"; widgets[a].changed(); }
      else if (op === "scroll") cm.scrollTo(null, a);
      else if (op === "edit") cm.replaceRange(b, { line: a, ch: 0 }, c ? { line: c, ch: 0 } : { line: a, ch: 0 });
      const events = log.slice();
      // signalLater outside an operation (clear()) is delivered by setTimeout(fireOrphanDelayed, 0).
      await new Promise(r => setTimeout(r, 0));
      const late = log.slice(events.length);
      const lines = [0, 1, 2, 3, 5, 10, 20, 39];
      recs.push({
        step: st, updates: updates - before, events, late,
        docHeight: cm.doc.height, scrollTop: cm.getScrollInfo().top,
        tops: lines.map(n => [n, cm.heightAtLine(n, "local"), cm.heightAtLine(n, "local", true)]),
        cursor: lines.map(n => { const c = cm.cursorCoords({ line: n, ch: 2 }, "local"); return [n, c.left, c.top, c.bottom]; }),
        coordsChar: lines.map(n => { const c = cm.cursorCoords({ line: n, ch: 2 }, "local"); const p = cm.coordsChar({ left: c.left + 1, top: c.top + 1 }, "local"); return [n, p.line, p.ch]; }),
        lineAtHeight: [0, 20, 40, 50, 80, 120, 200, 400, 900].map(y => [y, cm.lineAtHeight(y, "local")]),
        widgetCounts: lines.map(n => [n, ((cm.lineInfo(n) || {}).widgets || []).length]),
        order: [0, 1, 2, 3].map(n => (cm.getLineHandle(n).widgets || []).map(w => widgets.indexOf(w)))
      });
    }
    results.cases.push({ name, opts, recs });
    cm.getWrapperElement().remove();
  };
  await run("below_and_above", [["add", 1, 20], ["add", 2, 30, { above: true }], ["add", 2, 10], ["clear", 0], ["change", 1, 45], ["change", 1, 45], ["clear", 1], ["clear", 1]]);
  await run("insert_at", [["add", 0, 5], ["add", 0, 6, { insertAt: 0 }], ["add", 0, 7, { insertAt: 1 }], ["add", 0, 8, { insertAt: 99 }], ["add", 0, 9, { insertAt: -3 }]]);
  await run("above_visible_scroll", [["scroll", 380], ["add", 3, 25], ["add", 3, 15, { above: true }], ["change", 0, 50], ["clear", 1], ["add", 30, 40]]);
  await run("line_handle_and_edits", [["add", 5, 20], ["edit", 0, "new\n"], ["edit", 3, "", 8], ["add", 2, 11, { above: true }]]);
  await run("wrapping", [["add", 1, 20], ["add", 1, 12, { above: true }], ["change", 0, 33]], { lineWrapping: true });
  // Widget boxes (positionLineWidget, update_line.js:177) and clicks on widgets (eventInWidget, widgets.js:20).
  results.boxes = [];
  for (const scrollLeft of [0, 120]) {
    const long = Array.from({ length: 12 }, (_, i) => "line " + i + " " + "x".repeat(i == 4 ? 60 : 5)).join("\n");
    const cm = CodeMirror(document.body, { value: long, lineNumbers: true, gutters: ["g16"] });
    cm.setSize(600, 300);
    const specs = [[1, 20, {}], [2, 30, { coverGutter: true }], [3, 24, { noHScroll: true }], [5, 18, { coverGutter: true, noHScroll: true }], [0, 22, { above: true, handleMouseEvents: true }]];
    const ws = specs.map(([l, h, o]) => cm.addLineWidget(l, node(h), o));
    cm.scrollTo(scrollLeft, 0);
    await new Promise(r => setTimeout(r, 50));
    const box = cm.getWrapperElement().getBoundingClientRect();
    const rects = ws.map(w => { const r = w.node.parentNode.getBoundingClientRect(); return [r.left - box.left, r.right - box.left, r.top - box.top + cm.getScrollInfo().top - 4, r.bottom - box.top + cm.getScrollInfo().top - 4]; });
    const clicks = [];
    for (const [i, dx] of [[0, 10], [1, 5], [2, 60], [4, 80]]) {
      const r = rects[i];
      cm.setCursor({ line: 0, ch: 0 });
      const x = box.left + Math.max(r[0], 0) + dx, y = box.top + (r[2] + r[3]) / 2 + 4 - cm.getScrollInfo().top;
      const target = document.elementFromPoint(x, y);
      if (!target) throw new Error(`no element at ${x},${y} box ${JSON.stringify(box)} rect ${r}`);
      target.dispatchEvent(new MouseEvent("mousedown", { clientX: x, clientY: y, button: 0, bubbles: true, cancelable: true, view: window }));
      document.dispatchEvent(new MouseEvent("mouseup", { clientX: x, clientY: y, bubbles: true }));
      clicks.push([i, dx, cm.getCursor().line, cm.getCursor().ch]);
    }
    results.boxes.push({ scrollLeft, actualScrollLeft: cm.getScrollInfo().left, gutters: cm.display.gutters.offsetWidth, specs, rects, clicks });
    cm.getWrapperElement().remove();
  }
  return results;
});
} finally {
  await browser?.close();
  server.close();
}
console.log(out.cases.length + " cases");
if (OUT) await writeFile(OUT, JSON.stringify(out, null, 1) + "\n");
