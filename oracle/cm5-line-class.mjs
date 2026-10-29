/**
 * Line classes (issue #35: addLineClass / removeLineClass / lineInfo, src/model/Doc.js:265-291, the line DOM of
 * src/display/update_line.js) recorded from CodeMirror 5.65.21 in a headless browser.
 *   model: each step's thrown error name, redraw ("update" events) and the four class properties of lineInfo.
 *   dom:   the structure of the rendered line nodes and the measurement/hit-testing around them.
 * Measurement is normalized to cm_view::Metrics::oracle() as in cm5-highlight-worker.mjs.
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build CM5_PLAYWRIGHT_FROM=<dir>/ [CM5_BROWSER=chromium|firefox|webkit] \
 *     node oracle/cm5-line-class.mjs --out out.json
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
<style>body { margin: 0 } .CodeMirror { font-family: "DejaVu Sans Mono"; font-size: 32px; line-height: 38px }</style>`;
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
  const results = { version: CodeMirror.version, model: {}, dom: {} };
  // A step is [op, where, cls]: op "add", "rm" (cls null clears the property).
  const slot = v => v === undefined ? "undefined" : v;
  const info = cm => { const i = cm.lineInfo(0); return { text: slot(i.textClass), bg: slot(i.bgClass), wrap: slot(i.wrapClass), gutter: slot(cm.getLineHandle(0).gutterClass) }; }; // lineInfo has no gutterClass key; it is a property of the handle
  const model = (name, steps, doc = "one\ntwo") => {
    const cm = CodeMirror(document.body, { value: doc });
    cm.setSize(600, 300);
    let updates = 0;
    cm.on("update", () => updates++);
    const recs = [];
    for (const [op, where, cls] of steps) {
      const before = updates;
      let err = null, ret = null;
      try { ret = (op === "add" ? cm.addLineClass(0, where, cls) : cm.removeLineClass(0, where, cls)) ? "line" : String(ret); } catch (e) { err = e.name; }
      cm.getWrapperElement().getBoundingClientRect(); // flush nothing: updates run synchronously at the end of the operation
      recs.push({ step: [op, where, cls], err, ret, redraw: updates - before, ...info(cm) });
    }
    results.model[name] = recs;
    cm.getWrapperElement().remove();
  };
  const A = (where, cls) => ["add", where, cls], R = (where, cls) => ["rm", where, cls];
  const T = "text";
  model("empty_on_empty", [A(T, ""), A(T, ""), A(T, "x")]);
  model("empty_after_class", [A(T, "foo"), A(T, ""), A(T, ""), R(T, "")]);
  model("empty_after_two", [A(T, "foo"), A(T, "bar"), A(T, "")]);
  model("remove_empty_cls", [A(T, "foo bar"), R(T, ""), A(T, "x"), R(T, "")]);
  model("dot_wildcard", [A(T, "foo"), A(T, "f.o"), R(T, "f.o")]);
  model("alternation_remove", [A(T, "x"), A(T, "foo"), A(T, "y"), R(T, "foo|bar"), R(T, "x|y")]);
  model("alternation_add", [A(T, "foo"), A(T, "bar|foo"), A(T, "baz|qux")]);
  model("space_in_cls", [A(T, "a b"), A(T, "a b"), R(T, "a b"), A(T, "a"), A(T, "b"), R(T, "a b")]);
  model("duplicate_classes", [A(T, "a"), A(T, "a"), A(T, "b"), A(T, "a"), R(T, "a"), R(T, "a")]);
  model("remove_middle_first_last", [A(T, "a"), A(T, "b"), A(T, "c"), R(T, "b"), R(T, "a"), R(T, "c")]);
  model("invalid_open_bracket_nonempty", [A(T, "foo"), A(T, "["), R(T, "[")]);
  model("invalid_open_bracket_empty_slot", [A(T, "["), R(T, "[")]);
  model("invalid_open_paren_nonempty", [A(T, "foo"), A(T, "("), R(T, "(")]);
  model("invalid_star_nonempty", [A(T, "foo"), A(T, "*"), R(T, "*")]);
  model("invalid_backslash_nonempty", [A(T, "foo"), A(T, "\\"), R(T, "\\")]);
  model("valid_metachars", [A(T, "a.b"), A(T, "a+b"), R(T, "a\\.b"), R(T, "a+b"), A(T, "$"), A(T, "^x"), R(T, "^x"), R(T, "$")]);
  model("zero_width", [A(T, "foo"), A(T, "(?:)"), R(T, "(?:)"), R(T, "(?:)")]);
  model("lookahead", [A(T, "foo bar"), R(T, "foo(?= bar)"), R(T, "(?=bar)")]);
  model("whitespace_class", [A(T, "a"), A(T, "b"), R(T, "\\S+"), R(T, "\\S+")]);
  model("feff_separator", [A(T, "a﻿b"), A(T, "b"), R(T, "b")]);
  model("nel_separator", [A(T, "a\u0085b"), A(T, "b"), R(T, "b")]);
  model("tab_newline_separator", [A(T, "a\tb"), A(T, "b"), A(T, "a\nb"), R(T, "b")]);
  model("nbsp_separator", [A(T, "a b"), A(T, "b"), R(T, "b")]);
  model("remove_null_clears", [A(T, "a"), A(T, "b"), R(T, null), R(T, null), A(T, "c")]);
  model("remove_missing", [R(T, "a"), R(T, null), A(T, "a"), R(T, "b"), R(T, "ab")]);
  model("case_sensitive", [A(T, "Foo"), A(T, "foo"), R(T, "FOO"), R(T, "Foo")]);
  model("astral", [A(T, "\u{1F600}"), A(T, "\u{1F600}x"), R(T, "\u{1F600}"), R(T, "."), R(T, "..")]);
  model("where_variants", [A("text", "t"), A("background", "b"), A("wrap", "w"), A("gutter", "g"), A("other", "o"), A("", "e"), R("background", "b"), R("gutter", "g"), R("other", "o")]);
  model("add_after_clear_slot_empty_string", [A(T, ""), A(T, "foo"), R(T, "foo"), A(T, "")]);

  // ---- DOM structure and measurement --------------------------------------------------------------
  const code = cm => cm.getWrapperElement().querySelector(".CodeMirror-code");
  const ser = n => {
    if (n.nodeType !== 1) return null;
    const o = { t: n.tagName.toLowerCase(), c: n.getAttribute("class"), ce: n.getAttribute("contenteditable") };
    const st = n.getAttribute("style"); if (st) o.s = st.replace(/\s+/g, " ").trim();
    if (o.t !== "pre") o.k = Array.from(n.childNodes).map(ser).filter(Boolean);
    return o;
  };
  const dom = (name, opts, steps, doc = "alpha beta\ngamma\ndelta epsilon") => {
    const cm = CodeMirror(document.body, { value: doc, ...opts });
    cm.setSize(600, 300);
    const recs = [];
    const snap = label => {
      const c = code(cm);
      const lines = Array.from(c.children).map(ser);
      const m = [0, 1, 2].map(l => { const a = cm.cursorCoords({ line: l, ch: 0 }, "local"), b = cm.cursorCoords({ line: l, ch: 3 }, "local"); return [a.left, a.top, a.bottom, b.left, b.top, b.bottom].map(Math.round); });
      const cc = [0, 1, 2].map(l => { const r = cm.coordsChar({ left: 70, top: l * 38 + 19 }, "local"); return [r.line, r.ch]; });
      const h = [0, 38, 76, 114].map(y => cm.lineAtHeight(y, "local"));
      recs.push({ label, lines, coords: m, coordsChar: cc, lineAtHeight: h, height: cm.getScrollInfo().height, lineText: Array.from(c.querySelectorAll("pre.CodeMirror-line")).map(p => p.textContent.replace(/​/g, "")) });
    };
    snap("initial");
    for (const [op, line, where, cls] of steps) {
      if (op === "add") cm.addLineClass(line, where, cls);
      else if (op === "rm") cm.removeLineClass(line, where, cls);
      else if (op === "sel") { cm.setSelection({ line: 0, ch: 2 }, { line: 2, ch: 4 }); }
      else if (op === "cursor") cm.setCursor(line, 3);
      else if (op === "edit") cm.replaceRange("Z", { line, ch: 0 });
      else if (op === "refresh") cm.refresh();
      snap([op, line, where, cls].join(" "));
    }
    if (steps.some(s => s[0] === "sel")) {
      const rects = Array.from(cm.getWrapperElement().querySelectorAll(".CodeMirror-selected")).map(e => { const b = e.getBoundingClientRect(), r = cm.getWrapperElement().getBoundingClientRect(); return [Math.round(b.left - r.left), Math.round(b.top - r.top), Math.round(b.width), Math.round(b.height)]; });
      recs[recs.length - 1].selection = rects;
    }
    results.dom[name] = recs;
    cm.getWrapperElement().remove();
  };
  const D = [["add", 1, "text", "tx"], ["add", 1, "background", "bg"], ["add", 1, "wrap", "wr"]];
  dom("plain", {}, []);
  dom("text_only", {}, [["add", 1, "text", "tx"], ["rm", 1, "text", "tx"]]);
  dom("background_only", {}, [["add", 1, "background", "bg"], ["sel"], ["rm", 1, "background", "bg"]]);
  dom("wrap_only", {}, [["add", 1, "wrap", "wr"], ["rm", 1, "wrap", "wr"]]);
  dom("all_three", {}, [...D, ["sel"], ["cursor", 1], ["rm", 1, "wrap", "wr"], ["rm", 1, "background", "bg"], ["rm", 1, "text", "tx"]]);
  dom("empty_class_slots", {}, [["add", 1, "background", ""], ["add", 1, "wrap", ""], ["add", 1, "text", ""]]);
  dom("bg_and_edit", {}, [["add", 0, "background", "bg"], ["edit", 0], ["add", 2, "wrap", "wr"], ["refresh"]]);
  dom("line_numbers_plain", { lineNumbers: true }, []);
  dom("line_numbers_bg_wrap", { lineNumbers: true }, [...D, ["sel"]]);
  dom("line_wrapping", { lineWrapping: true }, [["add", 0, "background", "bg"], ["add", 0, "wrap", "wr"]], "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega\ngamma");
  return results;
});
} finally {
  await browser?.close();
  server.close();
}
console.log(JSON.stringify(out, null, 1).slice(0, 600));
if (OUT) await writeFile(OUT, JSON.stringify(out, null, 1) + "\n");
