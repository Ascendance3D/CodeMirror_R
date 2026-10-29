/**
 * htmlmixed resolves a tag's inner mode when the tag closes (issue #59, G13 #13: `html` in
 * mode/htmlmixed/htmlmixed.js:88-113 calls CodeMirror.getMode at token time), so a MIME type or mode defined
 * after the htmlmixed mode was created applies. Recorded from CodeMirror 5.65.21 in a headless browser: the
 * tokens ([text, style]) of each line, tokenized with one mode object made before the definitions in `defs`.
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build CM5_PLAYWRIGHT_FROM=<dir>/ [CM5_BROWSER=chromium|firefox|webkit] \
 *     node oracle/cm5-htmlmixed-late-mime.mjs --out out.json
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
const server = createServer(async (req, res) => {
  let rel;
  try { rel = decodeURIComponent(new URL(req.url, "http://x").pathname); } catch { res.writeHead(400); return res.end(); }
  if (rel === "/__harness.html") { res.writeHead(200, { "Content-Type": "text/html" }); return res.end("<!doctype html><meta charset=\"utf-8\">"); }
  const path = normalize(join(ROOT, rel)), r = relative(ROOT, path);
  if (r === ".." || r.startsWith(".." + sep) || isAbsolute(r)) { res.writeHead(403); return res.end(); }
  try { const body = await readFile(path); res.writeHead(200, { "Content-Type": { ".js": "text/javascript", ".css": "text/css" }[extname(path)] || "application/octet-stream" }); res.end(body); }
  catch { res.writeHead(404); res.end(); }
});
await new Promise(r => server.listen(0, "127.0.0.1", r));
let browser, out;
try {
browser = await pw[engine].launch({ headless: true });
const page = await browser.newPage();
await page.goto(`http://127.0.0.1:${server.address().port}/__harness.html`);
await page.addScriptTag({ url: "/lib/codemirror.js" });
await page.waitForFunction(() => window.CodeMirror);
for (const m of ["xml", "javascript", "css", "htmlmixed"]) await page.addScriptTag({ url: `/mode/${m}/${m}.js` });

out = await page.evaluate(() => {
  const results = { version: CodeMirror.version, cases: {} };
  const tokens = (mode, lines) => {
    const st = CodeMirror.startState(mode);
    return lines.map(text => {
      const res = [], stream = new CodeMirror.StringStream(text, 2);
      while (!stream.eol()) { const style = mode.token(stream, st); res.push([stream.current(), style === undefined ? null : style]); stream.start = stream.pos; }
      return res;
    });
  };
  // defs: ["mime", name, target] | ["mode", name] (a mode whose token styles the rest of the line "late-<name>").
  const define = defs => { for (const [kind, name, target] of defs) {
    if (kind === "mime") CodeMirror.defineMIME(name, target);
    else CodeMirror.defineMode(name, () => ({ token: s => { s.skipToEnd(); return "late-" + name; } }));
  } };
  const run = (name, spec, defs, lines) => {
    // Tag rule patterns are written as regexp sources.
    const tags = {};
    for (const t in spec.tags) tags[t] = spec.tags[t].map(([a, re, m]) => [a, re === null ? null : new RegExp(re), m]);
    const mode = CodeMirror.getMode({ indentUnit: 2 }, Object.assign({}, spec, { tags }));
    const before = tokens(mode, lines);
    define(defs);
    results.cases[name] = { spec, defs, lines, before, after: tokens(mode, lines) };
  };
  // G13 #13.
  run("late_mime", { name: "htmlmixed", tags: { script: [["type", "^text/x-late$", "text/x-late"], [null, null, "javascript"]] } },
    [["mime", "text/x-late", "javascript"]], ['<script type="text/x-late">var a</script>']);
  // A MIME type redefined after the mode was made.
  run("redefined_mime", { name: "htmlmixed", tags: { script: [["type", "^text/x-redef$", "text/x-redef"]] } },
    [["mime", "text/x-redef", "css"], ["mime", "text/x-redef", "javascript"]], ['<script type="text/x-redef">var b</script>']);
  // A mode defined after the mode was made, named directly in a tag rule.
  run("late_mode", { name: "htmlmixed", tags: { tpl: [[null, null, "latemode"]] } },
    [["mode", "latemode"]], ["<tpl>x y</tpl>", "<b>z</b>"]);
  // A MIME type given to a mode defined later.
  run("late_mime_to_late_mode", { name: "htmlmixed", tags: { script: [["type", "^text/x-two$", "text/x-two"]] } },
    [["mode", "secondmode"], ["mime", "text/x-two", "secondmode"]], ['<script type="text/x-two">q</script>']);
  return results;
});
} finally {
  await browser?.close();
  server.close();
}
console.log(JSON.stringify(out).slice(0, 300));
if (OUT) await writeFile(OUT, JSON.stringify(out, null, 1) + "\n");
