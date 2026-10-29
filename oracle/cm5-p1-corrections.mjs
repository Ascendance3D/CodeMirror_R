/**
 * P1 F1-F4 correction oracle for CodeMirror 5.65.21 in headless Chromium: the 25
 * independent QA probes of 2026-10-26-p1-final-qa-5adc052 (p01-p12d, unchanged),
 * plus regLineChange (f1_*) and maybeUpdateLineNumberWidth/resetView (f3_*) cases.
 * Same page metrics as cm5-viewport-matrix.mjs (cm_view::Metrics::oracle(): DejaVu
 * Sans Mono 32px, line-height 38px, padding 4px; setSize(600, 300)).
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build node cm5-p1-corrections.mjs --out out.json
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
  const s = safeResolve(url.pathname, ROOT);
  if (s.status !== 200) { res.writeHead(s.status); return res.end(); }
  const path = s.path;
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


// QA probes (independent, 2026-10-26 P1 final QA at 5adc052). Each probe is a fresh
// 600x300 editor over 200 lines, scrolled to `top`, then a list of steps; after each
// step (and a 60 ms tick) the state and the log since the previous step are recorded.
const out = await page.evaluate(async () => {
  const Pos = CodeMirror.Pos;
  const tick = () => new Promise(r => setTimeout(r, 60));
  const lines = n => Array.from({ length: n }, (_, i) => "line " + i).join("\n");
  const p = c => c.line + ":" + c.ch;
  const state = cm => {
    const s = cm.getScrollInfo(), vp = cm.getViewport();
    const first = cm.lineAtHeight(s.top, "local"), last = cm.lineAtHeight(s.top + s.clientHeight, "local");
    return { vp: [vp.from, vp.to], top: s.top, left: s.left, lines: cm.lineCount(),
             visible: [first, last], firstText: cm.getLine(first),
             gutter: cm.getGutterElement().offsetWidth };
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
  async function run(name, steps, { opts = {}, value, top = 3000, setup = null } = {}) {
    const cm = make(opts, value);
    if (setup) setup(cm);
    cm.scrollTo(null, top);
    await tick();
    const log = [];
    watch(cm, log, "");
    const rec = { before: state(cm), steps: [] };
    for (const [label, act] of steps) {
      await act(cm);
      await tick();
      rec.steps.push({ label, state: state(cm), log: log.splice(0) });
    }
    results.cases[name] = rec;
    cm.getWrapperElement().remove();
  }
  const op = (cm, f) => cm.operation(f);
  const rr = (cm, t, a, b) => cm.replaceRange(t, a, b);
  const native = async (cm, y) => { cm.getScrollerElement().scrollTop = y; await tick(); };
  const long = "w".repeat(70);
  const thirty = Array.from({ length: 30 }, (_, i) => "n" + i).join("\n") + "\n";

  await run("p01_multi_insert_above", [["op", cm => op(cm, () => { rr(cm, "X\n", Pos(0, 0)); rr(cm, "Y\n", Pos(10, 0)); rr(cm, "Z\n", Pos(20, 0)); })]]);
  await run("p02_multi_delete_above", [["op", cm => op(cm, () => { rr(cm, "", Pos(0, 0), Pos(1, 0)); rr(cm, "", Pos(10, 0), Pos(11, 0)); rr(cm, "", Pos(20, 0), Pos(22, 0)); })]]);
  await run("p03a_replace_same_count_cross_top", [["rr", cm => rr(cm, "a\nb", Pos(67, 0), Pos(68, 7))]]);
  await run("p03b_replace_first_rendered_line", [["rr", cm => rr(cm, "ZZZZZZZ", Pos(68, 0), Pos(68, 7))]]);
  await run("p04a_insert_30_above", [["rr", cm => rr(cm, thirty, Pos(0, 0))]]);
  await run("p04b_delete_30_above", [["rr", cm => rr(cm, "", Pos(0, 0), Pos(30, 0))]]);
  await run("p05a_gutter_grows_above", [["rr", cm => rr(cm, "X\n", Pos(0, 0))]], { opts: { lineNumbers: true }, value: lines(99) });
  await run("p05b_gutter_grows_below", [["rr", cm => rr(cm, "\nX", Pos(98, 7))]], { opts: { lineNumbers: true }, value: lines(99) });
  await run("p05c_gutter_shrinks_above", [["rr", cm => rr(cm, "", Pos(0, 0), Pos(1, 0))]], { opts: { lineNumbers: true }, value: lines(100) });
  await run("p05d_gutter_same_inside", [["rr", cm => rr(cm, "X\n", Pos(80, 0))]], { opts: { lineNumbers: true } });
  await run("p06a_scroll_then_edit_op", [["op", cm => op(cm, () => { cm.scrollTo(null, 4000); rr(cm, "X\n", Pos(0, 0)); })]]);
  await run("p06b_edit_inside_then_scroll_op", [["op", cm => op(cm, () => { rr(cm, "X\n", Pos(80, 0)); cm.scrollTo(null, 1000); })]]);
  await run("p07a_scroll_op_1px", [["scroll", cm => cm.scrollTo(null, 3001)]]);
  await run("p07b_scroll_op_half_line", [["scroll", cm => cm.scrollTo(null, 3019)]]);
  await run("p07c_native_scroll_1px", [["native", cm => native(cm, 3001)]]);
  await run("p09_direct_doc_two_changes", [["doc1", cm => cm.getDoc().replaceRange("X\n", Pos(0, 0))],
                                           ["doc2", cm => cm.getDoc().replaceRange("Y", Pos(100, 0))]]);
  await run("p10a_wrap_inside_shrinks", [["rr", cm => rr(cm, "", Pos(82, 0), Pos(82, 70))]],
            { opts: { lineWrapping: true }, setup: cm => cm.replaceRange(long, Pos(82, 0)) });
  await run("p10b_wrap_multi_above_grows", [["op", cm => op(cm, () => { rr(cm, long, Pos(0, 0)); rr(cm, long, Pos(1, 0)); rr(cm, long, Pos(2, 0)); })]],
            { opts: { lineWrapping: true } });
  await run("p11_rapid_alternating", [
    ["scroll3500", cm => cm.scrollTo(null, 3500)],
    ["insertAbove", cm => rr(cm, "X\n", Pos(0, 0))],
    ["scroll2500", cm => cm.scrollTo(null, 2500)],
    ["deleteAbove", cm => rr(cm, "", Pos(0, 0), Pos(1, 0))],
    ["scroll3000", cm => cm.scrollTo(null, 3000)],
    ["charInside", cm => rr(cm, "X", Pos(80, 0))],
    ["native3300", cm => native(cm, 3300)],
    ["insertInside", cm => rr(cm, "Q\n", Pos(90, 0))],
  ]);
  await run("p12a_retained_top_char_edit", [["scroll", cm => cm.scrollTo(null, 3000 + 4 * 38)],
                                            ["char", cm => rr(cm, "X", Pos(cm.getViewport().from, 0))]]);
  await run("p12b_retained_bottom_char_edit", [["scroll", cm => cm.scrollTo(null, 3000 - 4 * 38)],
                                               ["char", cm => rr(cm, "X", Pos(cm.getViewport().to - 1, 0))]]);
  await run("p12c_retained_top_mark_class", [["scroll", cm => cm.scrollTo(null, 3000 + 4 * 38)],
                                             ["mark", cm => cm.markText(Pos(cm.getViewport().from, 0), Pos(cm.getViewport().from, 2), { className: "q" })]]);
  await run("p12d_retained_top_insert_line", [["scroll", cm => cm.scrollTo(null, 3000 + 4 * 38)],
                                              ["line", cm => rr(cm, "X\n", Pos(cm.getViewport().from, 0))]]);
  // Linked editors at different scroll positions.
  {
    const a = make(), b = make({ value: a.getDoc().linkedDoc({ sharedHist: true }) });
    a.scrollTo(null, 3000); b.scrollTo(null, 1000); await tick();
    const log = []; watch(a, log, "A:"); watch(b, log, "B:");
    const rec = { before: { a: state(a), b: state(b) }, steps: [] };
    const step = async (label, f) => { f(); await tick(); rec.steps.push({ label, a: state(a), b: state(b), log: log.splice(0) }); };
    await step("A_insert_50", () => a.replaceRange("X\n", Pos(50, 0)));
    await step("B_insert_30", () => b.replaceRange("Y\n", Pos(30, 0)));
    await step("A_delete_10", () => a.replaceRange("", Pos(10, 0), Pos(11, 0)));
    results.cases.p08_linked_different_viewports = rec;
    a.getWrapperElement().remove(); b.getWrapperElement().remove();
  }
  // ---- F1: regLineChange keeps the rendered range (view_tracking.js:78).
  const up = 3000 + 4 * 38, down = 3000 - 4 * 38;
  const vf = cm => cm.getViewport().from, vt = cm => cm.getViewport().to;
  const mark = (cm, a, b) => cm.markText(Pos(a, 0), Pos(b, 2), { className: "q" });
  await run("f1_first_line_no_retention", [["char", cm => rr(cm, "X", Pos(vf(cm), 0))]]);
  await run("f1_last_line_no_retention", [["char", cm => rr(cm, "X", Pos(vt(cm) - 1, 0))]]);
  await run("f1_scroll_up1_first_line", [["scroll", cm => cm.scrollTo(null, 3000 + 38)],
                                         ["char", cm => rr(cm, "X", Pos(vf(cm), 0))]]);
  await run("f1_scroll_down1_last_line", [["scroll", cm => cm.scrollTo(null, 3000 - 38)],
                                          ["char", cm => rr(cm, "X", Pos(vt(cm) - 1, 0))]]);
  await run("f1_scroll_up4_last_line", [["scroll", cm => cm.scrollTo(null, up)],
                                        ["char", cm => rr(cm, "X", Pos(vt(cm) - 1, 0))]]);
  await run("f1_scroll_down4_first_line", [["scroll", cm => cm.scrollTo(null, down)],
                                           ["char", cm => rr(cm, "X", Pos(vf(cm), 0))]]);
  await run("f1_multi_line_edits_one_op", [["scroll", cm => cm.scrollTo(null, up)],
                                           ["op", cm => op(cm, () => { const a = vf(cm), b = vt(cm);
                                             rr(cm, "X", Pos(a, 0)); rr(cm, "Y", Pos(b - 1, 0)); rr(cm, "Z", Pos(a + 10, 0)); })]]);
  await run("f1_marker_multi_lines", [["scroll", cm => cm.scrollTo(null, up)],
                                      ["mark", cm => op(cm, () => mark(cm, vf(cm), vf(cm) + 2))]]);
  await run("f1_marker_across_top", [["scroll", cm => cm.scrollTo(null, up)],
                                     ["mark", cm => op(cm, () => mark(cm, vf(cm) - 2, vf(cm) + 1))]]);
  await run("f1_marker_add_remove_one_op", [["scroll", cm => cm.scrollTo(null, up)],
                                            ["op", cm => op(cm, () => mark(cm, vf(cm), vf(cm)).clear())]]);
  {
    let m = null;
    await run("f1_marker_clear_first_line", [["scroll", cm => cm.scrollTo(null, up)],
                                             ["mark", cm => { m = mark(cm, vf(cm), vf(cm)); }],
                                             ["scroll2", cm => cm.scrollTo(null, up + 2 * 38)],
                                             ["clear", cm => m.clear()]]);
  }
  await run("f1_line_edit_then_insert_one_op", [["scroll", cm => cm.scrollTo(null, up)],
                                                ["op", cm => op(cm, () => { rr(cm, "X", Pos(vf(cm), 0)); rr(cm, "N\n", Pos(0, 0)); })]]);
  await run("f1_insert_then_line_style_one_op", [["scroll", cm => cm.scrollTo(null, up)],
                                                 ["op", cm => op(cm, () => { const a = vf(cm); rr(cm, "N\n", Pos(0, 0)); mark(cm, a + 1, a + 1); })]]);
  await run("f1_insert_inside_then_line_edit_one_op", [["scroll", cm => cm.scrollTo(null, up)],
                                                       ["op", cm => op(cm, () => { const a = vf(cm); rr(cm, "N\n", Pos(a + 5, 0)); rr(cm, "X", Pos(a, 0)); })]]);
  await run("f1_whole_line_update_first_line", [["scroll", cm => cm.scrollTo(null, up)],
                                                ["empty", cm => rr(cm, "", Pos(vf(cm), 0))]]);
  // A full change (setValue: regChange(cm)). From the top, as Doc.setValue also
  // scrolls to 0,0; the setup scroll avoids the range drawn at the default height.
  const away = cm => cm.scrollTo(null, 2000);
  await run("f1_set_value_at_top", [["set", cm => cm.setValue(lines(200))]], { top: 0, setup: away });
  // Known difference (not F1-F4): Doc.setValue's scrollToCoords(cm, 0, 0).
  await run("n1_set_value_scrolled", [["scroll", cm => cm.scrollTo(null, up)],
                                      ["set", cm => cm.setValue(lines(200))]]);
  {
    const a = make(), b = make({ value: a.getDoc().linkedDoc({ sharedHist: true }) });
    a.scrollTo(null, up); b.scrollTo(null, 1000); await tick();
    b.scrollTo(null, 1000 + 4 * 38); await tick();
    const log = []; watch(a, log, "A:"); watch(b, log, "B:");
    const rec = { before: { a: state(a), b: state(b) }, steps: [] };
    const step = async (label, f) => { f(); await tick(); rec.steps.push({ label, a: state(a), b: state(b), log: log.splice(0) }); };
    await step("A_char_A_first", () => a.replaceRange("X", Pos(a.getViewport().from, 0)));
    await step("B_char_B_first", () => b.replaceRange("Y", Pos(b.getViewport().from, 0)));
    await step("A_char_B_last", () => a.replaceRange("Z", Pos(b.getViewport().to - 1, 0)));
    results.cases.f1_linked_retained = rec;
    a.getWrapperElement().remove(); b.getWrapperElement().remove();
  }
  // ---- F3: maybeUpdateLineNumberWidth runs after the first bail-out; a new digit
  // count resets the view (update_display.js:108).
  const ln = (extra = {}) => Object.assign({ lineNumbers: true }, extra);
  await run("f3_9_to_10_above", [["rr", cm => rr(cm, "X\n", Pos(0, 0))]], { opts: ln(), value: lines(9), top: 0 });
  await run("f3_10_to_9_above", [["rr", cm => rr(cm, "", Pos(0, 0), Pos(1, 0))]], { opts: ln(), value: lines(10), top: 0 });
  await run("f3_9_to_10_below", [["rr", cm => rr(cm, "\nX", Pos(8, 6))]], { opts: ln(), value: lines(9), top: 0 });
  await run("f3_999_to_1000_above", [["rr", cm => rr(cm, "X\n", Pos(0, 0))]], { opts: ln(), value: lines(999) });
  await run("f3_1000_to_999_above", [["rr", cm => rr(cm, "", Pos(0, 0), Pos(1, 0))]], { opts: ln(), value: lines(1000) });
  await run("f3_999_to_1000_below", [["rr", cm => rr(cm, "\nX", Pos(998, 8))]], { opts: ln(), value: lines(999) });
  await run("f3_99_to_100_unscrolled", [["rr", cm => rr(cm, "X\n", Pos(0, 0))]], { opts: ln(), value: lines(99), top: 0, setup: away });
  await run("f3_99_to_100_inside_unscrolled", [["rr", cm => rr(cm, "X\n", Pos(3, 0))]], { opts: ln(), value: lines(99), top: 0, setup: away });
  await run("f3_line_numbers_off", [["rr", cm => rr(cm, "X\n", Pos(0, 0))]], { value: lines(99) });
  await run("f3_first_line_number_large", [["rr", cm => rr(cm, "X\n", Pos(0, 0))]], { opts: ln({ firstLineNumber: 99990 }), value: lines(10), top: 0 });
  await run("f3_first_line_number_large_scrolled", [["rr", cm => rr(cm, "X\n", Pos(0, 0))]], { opts: ln({ firstLineNumber: 9902 }), value: lines(98) });
  await run("f3_nested_op", [["op", cm => op(cm, () => op(cm, () => rr(cm, "X\n", Pos(0, 0))))]], { opts: ln(), value: lines(99) });
  await run("f3_below_then_char", [["rr", cm => rr(cm, "\nX", Pos(98, 7))],
                                   ["char", cm => rr(cm, "X", Pos(80, 0))]], { opts: ln(), value: lines(99) });
  await run("f3_below_then_scroll", [["rr", cm => rr(cm, "\nX", Pos(98, 7))],
                                     ["scroll", cm => cm.scrollTo(null, 3000 + 12 * 38)]], { opts: ln(), value: lines(99) });
  return results;
});
const json = JSON.stringify(out, null, 1);
if (OUT) await writeFile(OUT, json + "\n"); else console.log(json);
await browser.close(); server.close();
