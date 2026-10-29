/**
 * StringStream past its start (issue #61, G13 #12: backUp in src/util/StringStream.js:47) recorded from
 * CodeMirror 5.65.21 in a headless browser. CM5 lets `pos` (and `start`) go negative; every later read goes
 * through the JS string semantics (charAt, slice, substr, indexOf). Each step records [op, result, pos, start];
 * `undefined` and `null` are recorded as the strings "undefined" and "null".
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build CM5_PLAYWRIGHT_FROM=<dir>/ [CM5_BROWSER=chromium|firefox|webkit] \
 *     node oracle/cm5-string-stream.mjs --out out.json
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

out = await page.evaluate(() => {
  const results = { version: CodeMirror.version, cases: {} };
  const norm = v => v === undefined ? "undefined" : v === null ? "null" : Array.isArray(v) ? v[0] : v;
  // Ops: "name" or "name:arg[:arg2]". "setStart" is `s.start = s.pos`.
  const apply = (s, op) => {
    const [name, a, b] = op.split(":");
    switch (name) {
      case "backUp": return s.backUp(Number(a));
      case "setStart": s.start = s.pos; return undefined;
      case "pos": return s.pos;
      case "current": return s.current();
      case "peek": return s.peek();
      case "next": return s.next();
      case "eol": return s.eol();
      case "sol": return s.sol();
      case "column": return s.column();
      case "indentation": return s.indentation();
      case "match": return s.match(a, b !== "false", false);
      case "matchRe": return s.match(new RegExp(a), b !== "false");
      case "eat": return s.eat(a === undefined ? "" : a);
      case "eatRe": return s.eat(new RegExp(a));
      case "eatWhile": return s.eatWhile(new RegExp(a));
      case "eatSpace": return s.eatSpace();
      case "skipTo": return s.skipTo(a);
      case "skipToEnd": return s.skipToEnd();
      default: throw new Error(op);
    }
  };
  const run = (name, text, ops) => {
    const s = new CodeMirror.StringStream(text, 4);
    results.cases[name] = { text, steps: ops.map(op => { const r = apply(s, op); return [op, norm(r), s.pos, s.start]; }) };
  };
  // The G13 #12 scenario of oracle/cm5-behavior-repro.mjs.
  run("g13_12", "abcdef", ["next", "setStart", "backUp:3", "pos", "current", "peek", "eol", "sol", "column",
    "match:ef:false", "matchRe:^e:false", "next", "next", "current", "eat:a", "next", "current"]);
  run("negative_start_current", "abcdef", ["backUp:2", "setStart", "current", "next", "current", "next", "next", "current", "column"]);
  run("negative_match_consume", "abcdef", ["backUp:2", "match:ef", "pos", "current", "eol", "peek"]);
  run("negative_match_re_consume", "abcdef", ["backUp:3", "matchRe:^d", "matchRe:^ef", "pos", "eol"]);
  run("negative_match_miss", "abcdef", ["backUp:1", "match:zz", "matchRe:^q", "match:F:false", "pos"]);
  run("negative_eat_variants", "abcdef", ["backUp:2", "eat:e", "eatRe:.", "eatWhile:.", "eat", "pos", "eat", "pos", "next", "pos"]);
  run("negative_eat_space", " \tab", ["backUp:2", "eatSpace", "pos", "next", "next", "eatSpace", "pos", "current"]);
  run("negative_skip_to", "abcabc", ["backUp:4", "skipTo:c", "pos", "backUp:9", "skipTo:z", "pos", "skipTo:a", "pos"]);
  run("negative_beyond_length", "ab", ["backUp:5", "current", "peek", "match:ab:false", "matchRe:^ab:false", "next", "pos", "current"]);
  run("negative_skip_to_end", "abcd", ["backUp:3", "setStart", "skipToEnd", "current", "eol", "indentation"]);
  run("back_up_to_zero", "abcd", ["next", "next", "backUp:2", "pos", "sol", "peek", "current"]);
  return results;
});
} finally {
  await browser?.close();
  server.close();
}
console.log(JSON.stringify(out).slice(0, 300));
if (OUT) await writeFile(OUT, JSON.stringify(out, null, 1) + "\n");
