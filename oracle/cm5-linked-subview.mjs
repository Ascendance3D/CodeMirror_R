/**
 * Linked-document and subview oracle for CodeMirror 5.65.21 in headless Chromium
 * (shiftDoc, ledger row 160). The cases are crates/cm-view/tests/data/linked-subview-cases.json,
 * which tests/linked_subview.rs reads too. Same page metrics as cm5-viewport-matrix.mjs
 * (cm_view::Metrics::oracle(): DejaVu Sans Mono 32px, line-height 38px; setSize(600, 300)).
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build node cm5-linked-subview.mjs --out out.json
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
const CASES = JSON.parse(await readFile(join(HERE, "../crates/cm-view/tests/data/linked-subview-cases.json"), "utf8"));
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

const out = await page.evaluate(async cases => {
  const Pos = CodeMirror.Pos;
  const tick = () => new Promise(r => setTimeout(r, 60));
  const lines = n => Array.from({ length: n }, (_, i) => "line " + i).join("\n");
  const p = c => c.line + ":" + c.ch;
  const TAGS = ["A:", "B:", "C:", "D:"];
  const state = cm => {
    const s = cm.getScrollInfo(), vp = cm.getViewport();
    const first = cm.lineAtHeight(s.top, "local"), last = cm.lineAtHeight(s.top + s.clientHeight, "local");
    return { vp: [vp.from, vp.to], top: s.top, left: s.left, lines: cm.lineCount(), first: cm.firstLine(),
             visible: [first, last], firstText: cm.getLine(first), lineNumChars: cm.display.lineNumChars,
             sels: cm.listSelections().map(r => p(r.anchor) + "-" + p(r.head)) };
  };
  const results = { version: CodeMirror.version, cases: {} };
  for (const c of cases.cases) {
    const log = [];
    const eds = [];
    const markers = [];
    const make = (i, value, numbers) => {
      const cm = CodeMirror(document.body, { value, lineNumbers: !!numbers });
      cm.setSize(600, 300);
      const ed = { cm, live: true };
      const tag = TAGS[i];
      const on = (ev, f) => cm.on(ev, (...a) => { if (ed.live) log.push(tag + f(...a)); });
      on("change", (_, ch) => `change(${p(ch.from)},${p(ch.to)},${JSON.stringify(ch.text)})`);
      on("changes", (_, cs) => `changes(${cs.length})`);
      on("update", () => "update");
      on("viewportChange", (_, a, b) => `viewportChange(${a},${b})`);
      on("cursorActivity", () => "cursorActivity");
      on("scroll", () => "scroll");
      return ed;
    };
    const a = make(0, lines(c.lines || 200), c.lineNumbers);
    eds.push(a);
    const docs = [a.cm.getDoc()];
    c.subs.forEach((s, k) => {
      const d = docs[0].linkedDoc({ sharedHist: true, from: s.from, to: s.to });
      docs.push(d);
      eds.push(make(k + 1, d, s.lineNumbers));
    });
    log.length = 0;
    for (const t of c.tops || [2000, 3000]) { a.cm.scrollTo(null, t); await tick(); }
    for (const [k, s] of c.subs.entries()) for (const t of s.tops) { eds[k + 1].cm.scrollTo(null, t); await tick(); }
    log.length = 0;
    const act = x => {
      const [k] = Object.keys(x), v = x[k];
      if (k === "rr") eds[v[0]].cm.replaceRange(v[1], Pos(...v[2]), v[3] ? Pos(...v[3]) : undefined);
      else if (k === "scroll") eds[v[0]].cm.scrollTo(null, v[1]);
      else if (k === "op") eds[v[0]].cm.operation(() => v[1].forEach(act));
      else if (k === "undo") eds[v].cm.undo();
      else if (k === "redo") eds[v].cm.redo();
      else if (k === "cursor") eds[v[0]].cm.setCursor(Pos(...v[1]));
      else if (k === "mark") markers.push(eds[v[0]].cm.markText(Pos(...v[1]), Pos(...v[2]), { className: "q" }));
      else if (k === "detach") { eds[v].live = false; eds[v].cm.swapDoc(CodeMirror.Doc("")); eds[v].cm.getWrapperElement().remove(); eds[v] = null; }
      else if (k === "attach") { const s = c.subs[v - 1]; eds[v] = make(v, docs[v], s.lineNumbers); }
      else if (k === "unlink") docs[v[0]].unlinkDoc(docs[v[1]]);
      else throw new Error("unknown action " + k);
    };
    const snap = () => ({ eds: eds.map(e => e && state(e.cm)),
                          markers: markers.map(m => { const f = m.find(); return f ? p(f.from) + "-" + p(f.to) : null; }) });
    const rec = { before: snap(), steps: [] };
    for (const [label, acts] of c.steps) {
      let error = null;
      try { acts.forEach(act); } catch (e) { error = String(e); }
      await tick();
      rec.steps.push(Object.assign({ label, log: log.splice(0) }, snap(), error ? { error } : {}));
    }
    results.cases[c.name] = rec;
    for (const e of eds) if (e) { e.live = false; e.cm.getWrapperElement().remove(); }
  }
  // A handler throws once in the operation that shifts B (A inserts above B): which
  // events of each editor are still delivered. `who` is the throwing editor and event.
  results.throws = {};
  for (const [who, ev] of [["A", "update"], ["B", "update"], ["A", "changes"], ["B", "viewportChange"]]) {
    const log = [];
    const a = CodeMirror(document.body, { value: lines(200) }); a.setSize(600, 300);
    const b = CodeMirror(document.body, { value: a.getDoc().linkedDoc({ sharedHist: true, from: 50, to: 150 }) }); b.setSize(600, 300);
    a.scrollTo(null, 2000); await tick(); a.scrollTo(null, 3000); await tick();
    b.scrollTo(null, 2000); await tick(); b.scrollTo(null, 400); await tick();
    let thrown = false;
    for (const [cm, tag] of [[a, "A:"], [b, "B:"]]) {
      for (const e of ["change", "changes", "update", "viewportChange"]) {
        cm.on(e, (_, x, y) => {
          log.push(tag + e + (e == "viewportChange" ? `(${x},${y})` : ""));
          if (!thrown && tag == who + ":" && e == ev) { thrown = true; throw new Error("boom"); }
        });
      }
    }
    let error = null;
    try { a.replaceRange("P\nQ\nR\n", Pos(10, 0)); } catch (e) { error = String(e); }
    await tick();
    const first = log.splice(0);
    const after = { a: [a.getViewport().from, a.getViewport().to], b: [b.getViewport().from, b.getViewport().to], bFirst: b.firstLine() };
    let nextError = null;
    try { b.replaceRange("X", Pos(53, 0)); } catch (e) { nextError = String(e); }
    await tick();
    results.throws[who + "_" + ev] = { error, log: first, after, next: log.splice(0), nextError };
    a.getWrapperElement().remove(); b.getWrapperElement().remove();
  }
  // Empty subview (from == to): recorded to classify, not compared.
  try {
    const d = CodeMirror.Doc(lines(200)).linkedDoc({ from: 50, to: 50 });
    results.empty_subview = { first: d.firstLine(), lines: d.lineCount(), value: d.getValue() };
  } catch (e) { results.empty_subview = { error: String(e) }; }
  return results;
}, CASES);
const json = JSON.stringify(out, null, 1);
if (OUT) await writeFile(OUT, json + "\n"); else console.log(json);
await browser.close(); server.close();
