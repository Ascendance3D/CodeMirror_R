/**
 * Highlighting overlays (issue #31: addOverlay / removeOverlay in src/edit/methods.js:67-87, the overlay pass of
 * highlightLine and Context.baseToken in src/line/highlight.js:31-102, getTokenTypeAt's "overlay " cut at
 * methods.js:140) recorded from CodeMirror 5.65.21 in a headless browser. Each step records every line's style
 * array (`line.styles` without the leading modeGen) and `styleClasses`, getTokenTypeAt at a few positions, the
 * redraws ("update" events) and an error, if any. The overlay modes are named in OVERLAYS; the Rust replay
 * implements each one.
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build CM5_PLAYWRIGHT_FROM=<dir>/ [CM5_BROWSER=chromium|firefox|webkit] \
 *     node oracle/cm5-overlays.mjs --out out.json
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
const HARNESS = `<!doctype html><meta charset="utf-8"><link rel="stylesheet" href="/lib/codemirror.css">`;
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
for (const m of ["xml", "javascript", "css", "htmlmixed"]) await page.addScriptTag({ url: `/mode/${m}/${m}.js` });

out = await page.evaluate(() => {
  const results = { version: CodeMirror.version, cases: {} };
  // The overlay modes. Mode objects are made fresh per case (removeOverlay compares them by identity).
  const OVERLAYS = {
    // test/test.js "getTokenTypeAt".
    foo: () => ({ token: s => { if (s.match("foo")) return "foo"; s.next(); } }),
    // test/test.js "addOverlay": baseToken().type.
    digits: () => ({ token: s => { const b = s.baseToken(); if (!/comment/.test(b.type) && s.match(/\d+/)) return "x"; s.next(); } }),
    // Splits the base tokens at word boundaries; named, so removeOverlay("ovw") finds it.
    word: () => ({ name: "ovw", token: s => { if (s.match(/^\w+/)) return "w"; s.next(); } }),
    // Skips a whole base string token with baseToken().size.
    instr: () => ({ token: s => { const b = s.baseToken(); if (b && b.type == "string") { s.pos += b.size; return "in-str"; } s.next(); } }),
    // Line classes from an overlay token, and a null-styled run.
    todo: () => ({ token: s => { if (s.match("TODO")) return "todo line-background-hl line-mark"; s.next(); return null; } }),
    // A style of only line classes: it becomes "" after extractLineClasses.
    lineonly: () => ({ token: s => { s.skipToEnd(); return "line-whole"; } }),
    // Records baseToken for each token: the type (with the "overlay ..." part cut) and size.
    probe: () => ({ token: s => { const b = s.baseToken(); s.next(); return "p-" + (b.type || "null").replace(/ /g, "_") + "-" + b.size; } }),
    // Has a startState: addOverlay throws.
    stateful: () => ({ startState: () => ({}), token: s => { s.skipToEnd(); } }),
  };
  CodeMirror.defineMode("ovl-digits", () => OVERLAYS.digits());
  CodeMirror.defineMode("ovl-word", () => OVERLAYS.word());
  const run = (name, mode, text, steps, probes = []) => {
    const cm = CodeMirror(document.body, { value: text, mode });
    cm.setSize(800, 400);
    let updates = 0;
    cm.on("update", () => updates++);
    const objs = {};
    const recs = [];
    for (const step of steps) {
      const [op, a, b] = step;
      const before = updates;
      let err = null;
      try {
        if (op === "add") {
          // a: overlay key, or {spec: <mode spec>}; b: options.
          let spec;
          if (typeof a === "string") spec = objs[a] = objs[a] || OVERLAYS[a]();
          else spec = objs[JSON.stringify(a.spec)] = a.spec;
          cm.addOverlay(spec, b);
        } else if (op === "remove") {
          // a: overlay key (the same object), {spec} (the object added), {name} (a string).
          if (typeof a === "string") cm.removeOverlay(objs[a] || OVERLAYS[a]());
          else if (a.spec !== undefined) cm.removeOverlay(objs[JSON.stringify(a.spec)] ?? a.spec);
          else cm.removeOverlay(a.name);
        } else if (op === "replace") cm.replaceRange(a, b[0], b[1]);
        else if (op === "setMode") cm.setOption("mode", a);
      } catch (e) { err = e.message; }
      const lines = [];
      for (let n = 0; n < cm.lineCount(); n++) {
        cm.getTokenTypeAt({ line: n, ch: 0 });
        const h = cm.getLineHandle(n);
        lines.push({ styles: h.styles.slice(1), classes: h.styleClasses ? { bgClass: h.styleClasses.bgClass || null, textClass: h.styleClasses.textClass || null } : null });
      }
      const types = probes.map(([l, ch]) => { const t = cm.getTokenTypeAt({ line: l, ch }); return t === undefined ? "undefined" : t; });
      recs.push({ step, err, redraw: updates - before, overlays: cm.state.overlays.length, lines, types });
    }
    results.cases[name] = { mode, text, probes, steps: recs };
    cm.getWrapperElement().remove();
  };
  run("get_token_type_at", "javascript", "1 + 'foo'", [["add", "foo"]], [[0, 0], [0, 6], [0, 7], [0, 9]]);
  run("opaque", "javascript", "1 + 'foo' + foo", [["add", "foo", { opaque: true }]], [[0, 6], [0, 13]]);
  run("base_token_comment", "javascript", "foo /* 100 */\nbar + 233;\nbaz", [["add", "digits"], ["replace", "", [{ line: 0, ch: 4 }, { line: 0, ch: 6 }]]], [[1, 7]]);
  run("non_opaque_splits", "javascript", "var abc = 'x y';", [["add", "word"]], [[0, 2], [0, 12]]);
  run("priority_order", "javascript", "foo 12 bar", [["add", "word", { priority: 2 }], ["add", "foo", { priority: 1 }], ["add", "digits"], ["remove", { name: "ovw" }], ["remove", "foo"], ["remove", "digits"]], [[0, 2], [0, 5]]);
  run("equal_priority_insert_after", "javascript", "foo 12", [["add", "digits", { priority: 1 }], ["add", "foo", { priority: 1 }], ["add", "word", { priority: 1, opaque: true }]], [[0, 2], [0, 5]]);
  run("opaque_then_non_opaque", "javascript", "foo = 'foo';", [["add", "foo", { opaque: true }], ["add", "word"]], [[0, 1], [0, 8]]);
  run("spec_string_and_name", "javascript", "a1 b22", [["add", { spec: "ovl-digits" }], ["add", { spec: { name: "ovl-word" } }], ["remove", { name: "nothing" }], ["remove", { name: "ovl-word" }], ["remove", { spec: "ovl-digits" }], ["remove", { spec: "ovl-digits" }]], [[0, 1]]);
  run("base_token_size", "javascript", "x = 'abc' + \"de\";", [["add", "instr"]], [[0, 6], [0, 14]]);
  run("line_classes", "javascript", "a TODO b\n\nTODO TODO", [["add", "todo"], ["add", "lineonly"]], [[0, 3]]);
  run("base_token_probe", "javascript", "if (a) 'b' // c", [["add", "foo", { opaque: true }], ["add", "probe"]], [[0, 1]]);
  run("base_token_probe_after_opaque", "javascript", "foofoo 1", [["add", "foo", { opaque: true }], ["add", "probe"]], [[0, 1]]);
  run("stateful_rejected", "javascript", "x", [["add", "stateful"], ["add", { spec: "javascript" }]], [[0, 0]]);
  run("keeps_overlays_on_set_mode", "javascript", "foo 1", [["add", "foo"], ["setMode", "css"]], [[0, 1]]);
  run("null_mode_overlay", null, "foo bar foo", [["add", "foo"]], [[0, 1], [0, 5]]);
  run("html_overlay", "htmlmixed", "<b>foo</b><script>foo(1)</script>", [["add", "foo"], ["add", "probe"]], [[0, 4], [0, 19]]);
  return results;
});
} finally {
  await browser?.close();
  server.close();
}
console.log(JSON.stringify(out).slice(0, 300));
if (OUT) await writeFile(OUT, JSON.stringify(out, null, 1) + "\n");
