/**
 * Cursor drawing (issue #51: prepareSelection and drawSelectionCursor, src/display/selection.js:13-60, with
 * showCursorWhenSelecting, cursorHeight and the cm-fat-cursor class) recorded from CodeMirror 5.65.21 in headless
 * Chromium. Same page metrics as cm5-scroll-api.mjs (cm_view::Metrics::oracle(): DejaVu Sans Mono 32px,
 * line-height 38px, padding 4px; setSize(600, 300)). Each cursor div's left/top/height (style, "div" coordinates)
 *, its style width (set only for the fat cursor) and its rendered width (offsetWidth: the 1px border of a normal cursor) are recorded.
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build CM5_PLAYWRIGHT_FROM=<dir>/ \
 *     node oracle/cm5-cursors.mjs --out crates/cm-view/tests/data/cm5-cursors.json
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
    const results = { version: CodeMirror.version, cases: {} };
    const docs = {
      short: "alpha\nbeta gamma\n\tdelta\nend",
      long: Array.from({ length: 60 }, (_, i) => `line ${i}`).join("\n"),
    };
    async function run(name, doc, opts, sel, fat = false) {
      const cm = CodeMirror(document.body, Object.assign({ value: docs[doc] }, opts)); cm.setSize(600, 300);
      if (fat) cm.getWrapperElement().classList.add("cm-fat-cursor");
      cm.focus();
      cm.setSelections(sel.map(([a, h]) => ({ anchor: Pos(a[0], a[1]), head: Pos(h[0], h[1]) })), 0, { scroll: false });
      await tick();
      const cursors = [...cm.getWrapperElement().querySelectorAll(".CodeMirror-cursors .CodeMirror-cursor")].map(c => ({
        left: parseFloat(c.style.left), top: parseFloat(c.style.top), height: parseFloat(c.style.height),
        width: c.offsetWidth, styleWidth: c.style.width ? parseFloat(c.style.width) : null }));
      results.cases[name] = { doc, opts, sel, fat, view: [cm.display.viewFrom, cm.display.viewTo], cursors };
      cm.getWrapperElement().remove();
    }
    await run("empty", "short", {}, [[[1, 3], [1, 3]]]);
    await run("tab", "short", {}, [[[2, 1], [2, 1]]]);
    await run("selection_hidden", "short", {}, [[[0, 1], [1, 4]]]);
    await run("selection_shown", "short", { showCursorWhenSelecting: true }, [[[0, 1], [1, 4]]]);
    await run("multi", "short", {}, [[[0, 0], [0, 0]], [[1, 2], [1, 6]], [[3, 3], [3, 3]]]);
    await run("multi_shown", "short", { showCursorWhenSelecting: true }, [[[0, 0], [0, 0]], [[1, 6], [1, 2]], [[3, 3], [3, 3]]]);
    await run("height_half", "short", { cursorHeight: 0.5 }, [[[1, 3], [1, 3]]]);
    await run("height_zero", "short", { cursorHeight: 0 }, [[[1, 3], [1, 3]]]);
    await run("fat", "short", {}, [[[1, 3], [1, 3]]], true);
    await run("fat_tab", "short", {}, [[[2, 0], [2, 0]]], true);
    await run("fat_eol", "short", {}, [[[0, 5], [0, 5]]], true);
    await run("fat_empty_line_doc", "short", { cursorHeight: 0.5 }, [[[3, 1], [3, 1]]], true);
    await run("out_of_view", "long", {}, [[[2, 0], [2, 0]], [[50, 1], [50, 1]]]);
    await run("range_into_view", "long", { showCursorWhenSelecting: true }, [[[50, 2], [3, 1]]]);
    return results;
  });
} finally {
  if (browser) await browser.close();
  server.close();
}
const json = JSON.stringify(out, null, 1) + "\n";
if (OUT) await writeFile(OUT, json); else process.stdout.write(json);
