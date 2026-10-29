/**
 * javascript `parserConfig.wordCharacters` (issue #58: `wordRE` in mode/javascript/javascript.js:21, used by
 * tokenBase at :105 and :122-123 and by findFatArrow at :204) recorded from CodeMirror 5.65.21 in a headless
 * browser. Each case tokenizes its lines with a fresh mode and state and records every token's text and style
 * and each line's `indent(state, "")`. A spec's wordCharacters string is a regexp source ("" is the empty regexp,
 * which matches everything); `false` stays false (falsy: the default applies).
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build CM5_PLAYWRIGHT_FROM=<dir>/ [CM5_BROWSER=chromium|firefox|webkit] \
 *     node oracle/cm5-javascript-word-chars.mjs --out out.json
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
  const run = (name, spec, lines) => {
    const built = Object.assign({}, spec);
    if (typeof built.wordCharacters === "string") built.wordCharacters = new RegExp(built.wordCharacters);
    const mode = CodeMirror.getMode({ indentUnit: 2 }, built);
    const state = CodeMirror.startState(mode);
    const recs = lines.map(line => {
      const toks = [], stream = new CodeMirror.StringStream(line, 4);
      while (!stream.eol()) {
        const style = mode.token(stream, state);
        toks.push([stream.current(), style === undefined ? null : style]);
        stream.start = stream.pos;
      }
      const ind = mode.indent(state, "");
      return { line, toks, indent: ind === CodeMirror.Pass ? "Pass" : ind };
    });
    results.cases[name] = { spec, lines: recs };
  };
  const text = ["var foo-bar = $x + café;", "let дом = a.b-c;", "(a-b, c) => a-b", "class K { #pri-v = 1 }", "x#y @z"];
  run("default", { name: "javascript" }, text);
  run("dash", { name: "javascript", wordCharacters: "[\\w$\\-]" }, text);
  run("ascii_only", { name: "javascript", wordCharacters: "[\\w$]" }, text);
  run("letters_only", { name: "javascript", wordCharacters: "[a-zA-Z]" }, text);
  run("cyrillic", { name: "javascript", wordCharacters: "[\\u0400-\\u04ff]" }, text);
  run("hash_and_at", { name: "javascript", wordCharacters: "[\\w#@]" }, text);
  run("empty_regexp", { name: "javascript", wordCharacters: "" }, text);
  run("falsy", { name: "javascript", wordCharacters: false }, text);
  run("typescript_dash", { name: "javascript", typescript: true, wordCharacters: "[\\w$\\-]" }, ["let a-b: my-type = c-d;", "type t-1 = x-y"]);
  return results;
});
} finally {
  await browser?.close();
  server.close();
}
console.log(JSON.stringify(out).slice(0, 300));
if (OUT) await writeFile(OUT, JSON.stringify(out, null, 1) + "\n");
