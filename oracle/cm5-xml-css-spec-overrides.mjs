/**
 * Spec overrides of the xml dictionaries (issue #59: `autoSelfClosers`, `implicitlyClosed`, `contextGrabbers`,
 * `doNotIndent` in mode/xml/xml.js:14-60, merged as `config[prop] = config_[prop]` over htmlConfig / xmlConfig)
 * and of css `tokenHooks` (mode/css/css.js:19, :42-45, merged by resolveMode over a MIME config) recorded from
 * CodeMirror 5.65.21 in a headless browser. Each case tokenizes its lines with a fresh mode and state and
 * records every token's text and style and, for xml, each line's indent(state, "") and indent(state, after).
 * Dictionaries are written as arrays of keys (contextGrabbers as [tag, [keys]] pairs) and built as objects
 * with `true` values; a non-array value is passed as is. The css hooks are named in HOOKS.
 *   CM5_BUILD_DIR=<disposable copy>/oracle/cm5-build CM5_PLAYWRIGHT_FROM=<dir>/ [CM5_BROWSER=chromium|firefox|webkit] \
 *     node oracle/cm5-xml-css-spec-overrides.mjs --out out.json
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
for (const m of ["xml", "css"]) await page.addScriptTag({ url: `/mode/${m}/${m}.js` });
out = await page.evaluate(() => {
  const results = { version: CodeMirror.version, xml: {}, css: {} };
  const dict = keys => Array.isArray(keys) ? Object.fromEntries(keys.map(k => [k, true])) : keys;
  const tokens = (mode, state, line) => {
    const toks = [], stream = new CodeMirror.StringStream(line, 4);
    while (!stream.eol()) {
      const style = mode.token(stream, state);
      toks.push([stream.current(), style === undefined ? "undefined" : style]);
      stream.start = stream.pos;
    }
    return toks;
  };
  const ind = v => v === CodeMirror.Pass ? "Pass" : v;
  const xml = (name, spec, lines, after) => {
    const built = Object.assign({ name: "xml" }, spec);
    for (const k of ["autoSelfClosers", "implicitlyClosed", "doNotIndent"]) if (k in built) built[k] = dict(built[k]);
    if (Array.isArray(built.contextGrabbers)) built.contextGrabbers = Object.fromEntries(built.contextGrabbers.map(([t, ks]) => [t, dict(ks)]));
    const mode = CodeMirror.getMode({ indentUnit: 2 }, built), state = CodeMirror.startState(mode);
    results.xml[name] = { spec, after, lines: lines.map(line => ({ line, toks: tokens(mode, state, line), indent: ind(mode.indent(state, "")), indentAfter: ind(mode.indent(state, after)) })) };
  };
  const HOOKS = {
    pct: s => { s.eatWhile(/[\w-]/); return ["variable-3", "variable"]; },
    slash: s => { if (s.eat("/")) { s.skipToEnd(); return ["comment", "comment"]; } return false; },
    tilde: () => "keyword",
    amp: () => null,
    caret: () => undefined,
  };
  const css = (name, spec, hooks, lines) => {
    const tokenHooks = {};
    for (const [ch, h] of hooks) tokenHooks[ch] = HOOKS[h];
    const mode = CodeMirror.getMode({ indentUnit: 2 }, Object.assign({}, spec, { tokenHooks })), state = CodeMirror.startState(mode);
    results.css[name] = { spec, hooks, lines: lines.map(line => ({ line, toks: tokens(mode, state, line) })) };
  };
  const ul = ["<ul>", "  <li>a", "    <li>b", "      </ul>", "<foo><bar>", "  </bar>", "    x"];
  xml("xml_default", {}, ul, "<li>");
  xml("xml_implicitly_closed", { implicitlyClosed: ["li"] }, ul, "<li>");
  xml("xml_context_grabbers", { contextGrabbers: [["li", ["li"]]] }, ul, "<li>");
  xml("xml_grabbers_and_implicit", { contextGrabbers: [["li", ["li"]]], implicitlyClosed: ["li"] }, ul, "</ul>");
  xml("xml_auto_self_closers", { autoSelfClosers: ["foo"] }, ul, "</foo>");
  xml("xml_do_not_indent", { doNotIndent: ["code"] }, ["<code>", "  <a>", "    x", "  </a>", "</code>", "<b>", "  y"], "</b>");
  const html = ["<div>", "  <br>", "  <pre>", "    <p>a", "      <p>b", "  </pre>", "  <input>", "    </div>", "z"];
  xml("html_default", { htmlMode: true }, html, "</div>");
  xml("html_empty_dicts", { htmlMode: true, autoSelfClosers: [], implicitlyClosed: [], contextGrabbers: [], doNotIndent: [] }, html, "</div>");
  xml("html_replaced_dicts", { htmlMode: true, autoSelfClosers: ["input"], doNotIndent: ["div"], contextGrabbers: [["p", ["p", "div"]]] }, html, "<p>");
  xml("html_do_not_indent_true", { htmlMode: true, doNotIndent: true }, html, "</div>");
  css("scss_replaced_hooks", { name: "text/x-scss" }, [["%", "pct"]], ["%ph { color: $x } // c", "#{a} b:hover {}"]);
  css("css_custom_hooks", { name: "text/css" }, [["/", "slash"], ["~", "tilde"], ["&", "amp"], ["^", "caret"]], ["a ~ b & c ^ d // x", "/* y */ e { color: red }"]);
  css("less_no_hooks", { name: "text/x-less" }, [], ["@var: 1; // c", ".a { &:hover { x: @var } }"]);
  css("css_without_tables", { name: "css" }, [["~", "tilde"]], ["a ~ b /* c */"]);
  return results;
});
} finally {
  await browser?.close();
  server.close();
}
console.log(JSON.stringify(out).slice(0, 300));
if (OUT) await writeFile(OUT, JSON.stringify(out, null, 1) + "\n");
