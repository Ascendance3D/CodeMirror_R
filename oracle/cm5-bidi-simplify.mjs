/**
 * bidiSimplify (issue #32: src/edit/mouse_events.js:337), recorded from the CodeMirror 5.65.21 sources in node.
 * The function is not exported, so its text is taken from the vendored oracle/cm5-build/src/edit/mouse_events.js
 * and run with the real getOrder / getBidiPartAt (src/util/bidi.js), cmp / Pos (src/line/pos.js) and Range
 * (src/model/selection.js). For each line text and document direction, every anchor (each ch, both stickies)
 * is combined with heads on the same line (each ch, both stickies) and on the lines above and below. The
 * order is getOrder(line) as bidiSimplify calls it (no direction argument: bidiOrdering(text, undefined)), or, in the
 * "cached" cases, the RTL order an RTL editor's rendering leaves in line.order (getOrder returns the cache).
 *   node oracle/cm5-bidi-simplify.mjs [out.json]
 */
import { readFile, writeFile } from "node:fs/promises";
import { join, normalize } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = normalize(join(fileURLToPath(import.meta.url), ".."));
const SRC = join(HERE, "cm5-build", "src");
const bidi = await import(pathToFileURL(join(SRC, "util", "bidi.js")));
const pos = await import(pathToFileURL(join(SRC, "line", "pos.js")));
const sel = await import(pathToFileURL(join(SRC, "model", "selection.js")));
const source = await readFile(join(SRC, "edit", "mouse_events.js"), "utf8");
const m = /\nfunction bidiSimplify\(cm, range\) \{[\s\S]*?\n\}\n/.exec(source);
if (!m) throw new Error("bidiSimplify not found");
const bidiSimplify = new Function("getLine", "getOrder", "getBidiPartAt", "cmp", "Range", "Pos",
  m[0] + "\nreturn bidiSimplify;")((doc, n) => doc.lines[n], bidi.getOrder, bidi.getBidiPartAt, pos.cmp, sel.Range, pos.Pos);

const TEXTS = ["abc", "אבג", "abאבcd", "אב xy גד", "aاب 12 ةb", "x א 1 2 ב y", ""];
const cases = [];
for (const text of TEXTS) for (const [direction, cached] of [["ltr", false], ["rtl", false], ["rtl", true]]) {
  const lines = [{ text: "zz" }, { text }, { text: "zz" }];
  const cm = { doc: { lines, direction } };
  const order = cached ? bidi.getOrder({ text }, "rtl") : bidi.getOrder({ text });
  const results = [];
  const P = (line, ch, sticky) => pos.Pos(line, ch, sticky);
  for (let ach = 0; ach <= text.length; ach++) for (const asticky of ["before", "after"]) {
    const heads = [P(0, 1, "after"), P(2, 0, "before")];
    for (let hch = 0; hch <= text.length; hch++) for (const hs of ["before", "after"]) heads.push(P(1, hch, hs));
    for (const head of heads) {
      const range = new sel.Range(P(1, ach, asticky), head);
      lines[1].order = cached ? order : undefined;
      const r = bidiSimplify(cm, range);
      results.push([ach, asticky, head.line, head.ch, head.sticky, r === range ? null : [r.anchor.ch, r.anchor.sticky]]);
    }
  }
  cases.push({ text, direction, cached, order: order ? order.map(p => [p.level, p.from, p.to]) : null, results });
}
const out = { version: "5.65.21", cases };
await writeFile(process.argv[2] || "cm5-bidi-simplify.json", JSON.stringify(out) + "\n");
console.log(cases.map(c => `${JSON.stringify(c.text)} ${c.direction}${c.cached ? " cached" : ""}: ${c.results.length} (${c.results.filter(r => r[5]).length} changed)`).join("\n"));
