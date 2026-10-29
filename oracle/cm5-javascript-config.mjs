/**
 * javascript mode parser options and mode-object functions (issue #58: mode/javascript/javascript.js:279
 * `register` with `parserConfig.globalVars`, :858 `expressionAllowed`, :867 `startState` with
 * `parserConfig.localVars` / `globalVars`, :940 `skipExpression`) recorded from CodeMirror 5.65.21 in a
 * headless browser. Each case tokenizes its lines with a fresh mode and state; after every token it records
 * the token, its style and `mode.expressionAllowed(stream, state)` / `(stream, state, 1)`; at the end of each
 * line the names in `state.localVars`, `state.globalVars`, whether `state.context` is set, and
 * `mode.indent(state, "")`. A line "@skip" calls `mode.skipExpression(state)` instead of tokenizing.
 * A var list in a spec is written as an array of names and built as `{name, next}` objects.
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build CM5_PLAYWRIGHT_FROM=<dir>/ [CM5_BROWSER=chromium|firefox|webkit] \
 *     node oracle/cm5-javascript-config.mjs --out out.json
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
await page.addScriptTag({ url: "/mode/javascript/javascript.js" });

out = await page.evaluate(() => {
  const results = { version: CodeMirror.version, cases: {} };
  const list = names => names.reduceRight((next, name) => ({ name, next }), null);
  const names = v => { const o = []; for (; v; v = v.next) o.push(v.name === undefined ? null : v.name); return o; };
  const run = (name, spec, lines) => {
    const built = Object.assign({}, spec);
    for (const k of ["localVars", "globalVars"]) if (Array.isArray(built[k])) built[k] = list(built[k]);
    const mode = CodeMirror.getMode({ indentUnit: 2 }, built);
    const state = CodeMirror.startState(mode);
    const recs = [];
    for (const line of lines) {
      const toks = [];
      if (line === "@skip") mode.skipExpression(state);
      else {
        const stream = new CodeMirror.StringStream(line, 4);
        while (!stream.eol()) {
          const style = mode.token(stream, state);
          toks.push([stream.current(), style === undefined ? null : style, !!mode.expressionAllowed(stream, state), !!mode.expressionAllowed(stream, state, 1)]);
          stream.start = stream.pos;
        }
      }
      const ind = mode.indent(state, "");
      recs.push({ line, toks, localVars: names(state.localVars), globalVars: state.globalVars === undefined ? null : names(state.globalVars), context: !!state.context, indent: ind === CodeMirror.Pass ? "Pass" : ind });
    }
    results.cases[name] = { spec, lines: recs };
  };
  run("plain", { name: "javascript" }, ["var a = b;", "a + c", "`x${", "y"]);
  run("local_vars", { name: "javascript", localVars: ["x", "y"] }, ["x + y + z;", "var q = x;", "q; function f(p) { return p + x }", "f(q)"]);
  run("global_vars_true", { name: "javascript", globalVars: true }, ["var g = 1, h;", "function k(a) { var inner; }", "let m; g + m"]);
  run("global_vars_list", { name: "javascript", globalVars: ["pre"] }, ["var g;", "var pre;", "pre + g"]);
  run("global_and_local", { name: "javascript", localVars: ["l"], globalVars: true }, ["var a;", "{ var b; let c; }", "a + b + l"]);
  run("global_vars_no_track_scope", { name: "javascript", globalVars: true, trackScope: false }, ["var a;", "a"]);
  run("skip_expression_after_assign", { name: "javascript" }, ["var a =", "@skip", "; b", "c"]);
  run("skip_expression_in_call", { name: "javascript" }, ["f(", "@skip", ", 2)", "x"]);
  run("skip_expression_statement", { name: "javascript" }, ["@skip", "+ 1", "if (", "@skip", ") y"]);
  run("expression_allowed_regexp", { name: "javascript" }, ["x = /a/; y / 2", "return /b/", "z /= 3"]);
  run("typescript_local", { name: "javascript", typescript: true, localVars: ["t"] }, ["let u: number = t;", "t + u"]);
  return results;
});
} finally {
  await browser?.close();
  server.close();
}
console.log(JSON.stringify(out).slice(0, 300));
if (OUT) await writeFile(OUT, JSON.stringify(out, null, 1) + "\n");
