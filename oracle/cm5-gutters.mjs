/**
 * Gutters (issue #36: getGutters/renderGutters/updateGutters src/display/gutters.js, updateGutterSpace, gutterEvent
 * src/edit/mouse_events.js:370-417) recorded from CodeMirror 5.65.21 in a headless browser: the resolved gutter specs,
 * each column's left/width, the gutters' total width (border included), and the gutterClick / gutterContextMenu
 * events (line, className, whether the click reached the text) for clicks at given window points. Page CSS:
 * .g16 { width: 16px } .g30 { width: 30px } .gw { width: 7px }.
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build CM5_PLAYWRIGHT_FROM=<dir>/ [CM5_BROWSER=chromium|firefox|webkit] \
 *     node oracle/cm5-gutters.mjs --out out.json
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
<style>body { margin: 0 } .CodeMirror { font-family: "DejaVu Sans Mono"; font-size: 32px; line-height: 38px } .g16 { width: 16px } .g30 { width: 30px } .gw { width: 7px }</style>`;
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
  const results = { version: CodeMirror.version, cases: [] };
  const doc = Array.from({ length: 30 }, (_, i) => "line " + i).join("\n");
  const run = (name, opts, clicks, after) => {
    const cm = CodeMirror(document.body, Object.assign({ value: doc }, opts));
    cm.setSize(600, 300);
    if (after) after(cm);
    const box = cm.getWrapperElement().getBoundingClientRect();
    const g = cm.display.gutters, gb = g.getBoundingClientRect();
    const rec = {
      name, opts: JSON.parse(JSON.stringify(opts)),
      charWidth: cm.defaultCharWidth(), textHeight: cm.defaultTextHeight(),
      specs: cm.display.gutterSpecs.map(s => [s.className, s.style]),
      columns: Array.from(g.childNodes).map(n => { const r = n.getBoundingClientRect(); return [n.className, r.left - gb.left, r.width]; }),
      guttersWidth: g.offsetWidth, sizerMargin: parseFloat(cm.display.sizer.style.marginLeft) || 0, events: []
    };
    const log = [];
    cm.on("gutterClick", (_, line, cls) => log.push(["click", line, cls]));
    if (opts.ctx) cm.on("gutterContextMenu", (_, line, cls, e) => { log.push(["ctx", line, cls]); e.preventDefault(); });
    for (const [x, y, scroll, button] of clicks) {
      if (scroll) { cm.scrollTo(null, scroll); cm.refresh(); }
      log.length = 0;
      const before = JSON.stringify(cm.getCursor());
      const type = button === 3 ? "contextmenu" : "mousedown";
      const ev = new MouseEvent(type, { clientX: box.left + x, clientY: box.top + y, button: button === 3 ? 2 : 0, bubbles: true, cancelable: true, view: window });
      const target = document.elementFromPoint(box.left + x, box.top + y);
      target.dispatchEvent(ev);
      if (type === "mousedown") document.dispatchEvent(new MouseEvent("mouseup", { clientX: box.left + x, clientY: box.top + y, bubbles: true }));
      rec.events.push({ x, y, scroll: scroll || 0, button: button || 1, log: log.slice(), defaultPrevented: ev.defaultPrevented, cursorMoved: JSON.stringify(cm.getCursor()) !== before, scrollTop: cm.getScrollInfo().top });
    }
    results.cases.push(rec);
    cm.getWrapperElement().remove();
  };
  const clicks = [[2, 5], [10, 20], [15, 50], [17, 50], [20, 80], [40, 80], [45, 100], [60, 120], [80, 290], [5, 100, 400], [200, 50]];
  run("none", {}, [[2, 5], [200, 50]]);
  run("line_numbers", { lineNumbers: true }, clicks);
  run("custom_only", { gutters: ["g16"] }, clicks);
  run("custom_and_numbers_appended", { gutters: ["g16", "g30"], lineNumbers: true }, clicks);
  run("numbers_placed_first", { gutters: ["CodeMirror-linenumbers", "g16"], lineNumbers: true }, clicks);
  run("numbers_dropped", { gutters: ["CodeMirror-linenumbers", "g30"], lineNumbers: false }, clicks);
  run("style_width", { gutters: [{ className: "gs", style: "width: 25px" }, "gw"] }, clicks);
  run("zero_width_class", { gutters: ["nowidth", "g16"] }, clicks);
  run("first_line_number", { lineNumbers: true, firstLineNumber: 995, gutters: ["g16"] }, clicks);
  run("context_menu", { gutters: ["g16", "g30"], ctx: true }, [[5, 50, 0, 3], [30, 50, 0, 3], [200, 50, 0, 3]]);
  run("set_option_gutters", { lineNumbers: true }, clicks, cm => cm.setOption("gutters", ["g30"]));
  run("set_option_line_numbers_off", { gutters: ["g16"], lineNumbers: true }, clicks, cm => cm.setOption("lineNumbers", false));
  return results;
});
} finally {
  await browser?.close();
  server.close();
}
console.log(out.cases.length + " cases");
if (OUT) await writeFile(OUT, JSON.stringify(out, null, 1) + "\n");
