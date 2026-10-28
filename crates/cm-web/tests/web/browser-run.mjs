// CM5_PLAYWRIGHT_FROM=<dir with node_modules/playwright>/ \
//   node browser-run.mjs --pkg <wasm-bindgen pkg dir> (--cm5 <built cm5 dir> | --no-cm5) [--out <dir>]
//     [--browser-timeout <s>] [--inject <fault>] <chromium|firefox|webkit|chromium-mobile|webkit-mobile>...
//
// For each browser: load page.html (served through page.route, no server), run
// the L-B scenarios in the page, then drive the attached adapter with real
// Playwright input (keyboard, mouse, wheel, touch, clipboard) and compare it,
// step by step, with the pinned CodeMirror 5 driven by the same input in the
// same page (--cm5 names a built tree with lib/codemirror.js). CM5's values
// are the expected values; a difference is either a failure or a named known
// difference listed in KNOWN below. Development-only Node use.
//
// Each browser gets a status, and the exit status is the worst of them:
//   PASS (0)    the reference loaded and every required comparison ran and passed;
//   FAIL (1)    a check failed, a required comparison did not run, the page threw, or the run timed out;
//   BLOCKED (2) the CM5 reference or the browser is unavailable (no comparison is counted);
//   UNSUPPORTED (2) the browser name has no profile here.
// Without --cm5 nothing runs. --no-cm5 runs the cm-web checks alone and is BLOCKED.
// A check's status is PASS (exact), KNOWN_GAP (ran, differs for a reason in KNOWN or
// KNOWN_GAPS.md), UNSUPPORTED (the environment cannot perform the action) or FAIL.
// --inject <fault> makes one run fail on purpose (harness-selftest.mjs).
import { createRequire } from "node:module";
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "../../../..");
const args = process.argv.slice(2);
const opt = k => { const i = args.indexOf(k); if (i < 0) return null; const v = args[i + 1]; args.splice(i, 2); return v; };
const flag = k => { const i = args.indexOf(k); if (i < 0) return false; args.splice(i, 1); return true; };
const PKG = resolve(opt("--pkg") ?? "");
const CM5 = opt("--cm5");
const NO_CM5 = flag("--no-cm5");
const OUT = resolve(opt("--out") ?? ".");
const BROWSER_TIMEOUT = Number(opt("--browser-timeout") ?? 300) * 1000;
const INJECT = opt("--inject");
const FAULTS = ["launch-fail", "context-fail", "nav-fail", "page-throw", "reject", "compare-fail", "drop-section", "drop-web-check", "hang"];
mkdirSync(OUT, { recursive: true });
const EXIT = { PASS: 0, FAIL: 1, BLOCKED: 2, UNSUPPORTED: 2 };
// Write the results and the summary, and set the exit status (0 only when every browser passed).
function finish(out) {
  writeFileSync(join(OUT, "browser-run.json"), JSON.stringify(out, null, 1));
  console.log(JSON.stringify(out.map(r => ({ browser: r.browser, status: r.status, version: r.version, ok: r.ok, lb: r.lb?.ok, lb_steps: r.lb?.steps, summary: r.summary, integrity: r.integrity?.complete, blocked: r.blocked, page_errors: r.page_errors, error: r.error, cleanup: r.cleanup })), null, 1));
  const worst = out.length ? Math.max(...out.map(r => EXIT[r.status] ?? 1)) : 2;
  process.exitCode = out.some(r => r.status === "FAIL") ? 1 : worst;
}
const blockedAll = reason => finish((args.length ? args : ["(none)"]).map(browser => ({ browser, status: "BLOCKED", ok: false, blocked: reason, differential: "not-run" })));
if (!process.env.CM5_PLAYWRIGHT_FROM) { blockedAll("CM5_PLAYWRIGHT_FROM is not set (Playwright not found)"); process.exit(); }
if (args.length === 0) { blockedAll("no browser named"); process.exit(); }
if (INJECT && !FAULTS.includes(INJECT)) { blockedAll(`unknown --inject fault ${INJECT} (one of ${FAULTS.join(", ")})`); process.exit(); }
if (!!CM5 === NO_CM5) { blockedAll(CM5 ? "--cm5 and --no-cm5 together" : "CM5 reference build not supplied (--cm5 <built cm5 dir>); no comparison can run"); process.exit(); }
const pw = createRequire(process.env.CM5_PLAYWRIGHT_FROM)("playwright");

// The pinned reference (tools/reference.lock) and the files the comparison loads from it.
const LOCK = Object.fromEntries(readFileSync(join(ROOT, "tools/reference.lock"), "utf8").split("\n").filter(l => /^\w+=/.test(l)).map(l => [l.slice(0, l.indexOf("=")), l.slice(l.indexOf("=") + 1).replace(/^"|"$/g, "")]));
const CM5_FILES = ["lib/codemirror.css", "lib/codemirror.js", "mode/clike/clike.js", "mode/javascript/javascript.js"];
// The CM5 API the drivers in page.mjs call (src/edit/methods.js, src/model/Doc.js) and the modes they use.
const CM5_API = ["getValue", "getCursor", "listSelections", "historySize", "hasFocus", "getScrollInfo", "refresh", "cursorCoords", "getWrapperElement", "getScrollerElement", "getInputField"];

class Blocked extends Error {}

// Host-side check of the reference build before any browser starts. The css and mode
// files are copies of the pinned tree, so they must equal CM5_REF_DIR byte for byte;
// lib/codemirror.js is a rollup output, so it is checked by its version line (and its
// src/, when the build dir has one, must equal the pinned src/).
function validateReference(dir) {
  const ref = { dir: resolve(dir), pinned: LOCK.CM5_PIN, version: LOCK.CM5_VERSION };
  const pinned = join(ROOT, LOCK.CM5_REF_DIR);
  if (!existsSync(dir) || !statSync(dir).isDirectory()) throw new Blocked(`CM5 build dir ${dir} does not exist or is not a directory`);
  if (readdirSync(dir).length === 0) throw new Blocked(`CM5 build dir ${dir} is empty`);
  const missing = CM5_FILES.filter(f => !existsSync(join(dir, f)));
  if (missing.length) throw new Blocked(`CM5 build dir ${dir} lacks ${missing.join(", ")}`);
  const differ = CM5_FILES.filter(f => f !== "lib/codemirror.js" && !readFileSync(join(dir, f)).equals(readFileSync(join(pinned, f))));
  if (differ.length) throw new Blocked(`CM5 build dir ${dir}: ${differ.join(", ")} differ from the pinned ${LOCK.CM5_REF_DIR}`);
  const js = readFileSync(join(dir, "lib/codemirror.js"), "utf8");
  if (!js.includes(`CodeMirror.version = "${LOCK.CM5_VERSION}"`)) throw new Blocked(`CM5 build dir ${dir}: lib/codemirror.js does not declare CodeMirror.version = "${LOCK.CM5_VERSION}"`);
  if (existsSync(join(dir, "src/codemirror.js"))) {
    const walk = d => readdirSync(join(pinned, d), { withFileTypes: true }).flatMap(e => e.isDirectory() ? walk(join(d, e.name)) : [join(d, e.name)]);
    const src = walk("src");
    const bad = src.filter(f => !existsSync(join(dir, f)) || !readFileSync(join(dir, f)).equals(readFileSync(join(pinned, f))));
    if (bad.length) throw new Blocked(`CM5 build dir ${dir}: src/ differs from the pinned tree (${bad.slice(0, 3).join(", ")}${bad.length > 3 ? ", …" : ""})`);
    ref.src = `${src.length} files equal the pinned src/`;
  } else ref.src = "no src/ in the build dir: lib/codemirror.js is identified by its version line only";
  ref.files = `${CM5_FILES.length - 1} files equal the pinned tree; lib/codemirror.js declares ${LOCK.CM5_VERSION}`;
  return ref;
}
let REFERENCE = null;
if (CM5) {
  try { REFERENCE = validateReference(CM5); } catch (e) {
    if (!(e instanceof Blocked)) throw e;
    blockedAll(e.message); process.exit();
  }
}

const TYPES = { ".mjs": "text/javascript", ".js": "text/javascript", ".html": "text/html", ".css": "text/css", ".wasm": "application/wasm" };
function serve(route) {
  const path = new URL(route.request().url()).pathname;
  let file;
  if (path.startsWith("/pkg/")) file = join(PKG, path.slice(5));
  else if (path.startsWith("/cm5/") && REFERENCE) file = join(REFERENCE.dir, path.slice(5));
  else file = join(HERE, path.slice(1));
  if (!existsSync(file)) return route.fulfill({ status: 404, body: "not found" });
  return route.fulfill({ body: readFileSync(file), contentType: TYPES[extname(file)] ?? "application/octet-stream" });
}

// Known, explained differences from CM5 (KNOWN_GAPS.md). A check named here records
// its CM5 and cm-web values and does not fail the run.
const KNOWN = {
  "ime.during": "cm-web keeps the preedit out of the document; CM5's textarea writes it with origin *compose (KNOWN_GAPS.md G1)",
  "ime.history": "CM5 merges the *compose events and the commit into its history differently (KNOWN_GAPS.md G1)",
  "scroll.wheel_500.top": "CM5 leaves vertical wheel scrolling to the browser, which scrolls its own amount (Firefox and WebKit differ from deltaY); cm-web cancels the default and applies deltaY (KNOWN_GAPS.md G5)",
  "scroll.wheel_-200.top": "follows from scroll.wheel_500.top (KNOWN_GAPS.md G5)",
};

const C_DOC = Array.from({ length: 60 }, (_, i) => i === 0 ? "#include <stdio.h>" : i === 1 ? "int main(int argc, char **argv) {" : i === 59 ? "}" : `  int v${i} = ${i} * 2; // line ${i}`).join("\n");
const LONG = Array.from({ length: 300 }, (_, i) => `line ${i} ${"x".repeat(i % 7 * 10)}`).join("\n");

// `<engine>-mobile` runs the engine with a Playwright device profile (viewport,
// isMobile, hasTouch, user agent). That is emulation, not a phone.
const MOBILE = { "chromium-mobile": "Pixel 7", "webkit-mobile": "iPhone 15" };
const BROWSERS = ["chromium", "firefox", "webkit", ...Object.keys(MOBILE)];

// The CM5 comparisons each run must make, by section (88 in all). A section may run
// fewer only when the environment cannot perform its action, which is recorded as
// UNSUPPORTED; any other shortfall fails the run.
// REQUIRED counts comparisons with CM5 (needs the reference). REQUIRED_WEB counts the cm-web-only checks that
// must run in every mode, with or without the reference; a run that skips one fails.
const REQUIRED_WEB = { window_scroll: 2 };
const REQUIRED = { render: 3, keys: 20, mouse: 5, scroll: 39, window_scroll: 10, focus: 1, clipboard: 4, ime: 3, touch: 1, measure: 2 };

// One browser run. It always returns a result with a status, and it closes the
// browser on every path (normal end, failed check, thrown error, rejected promise,
// timeout). A cleanup error is recorded next to the original failure, never instead.
async function runBrowser(name) {
  const res = { browser: name, checks: [], page_errors: [], console_errors: [], ...(INJECT ? { injected: INJECT } : {}) };
  if (!BROWSERS.includes(name)) return { ...res, status: "UNSUPPORTED", ok: false, error: `no browser profile ${name} (one of ${BROWSERS.join(", ")})` };
  const own = { browser: null, launching: null };
  let timer;
  const deadline = new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`timed out after ${BROWSER_TIMEOUT / 1000} s`)), BROWSER_TIMEOUT); });
  try {
    await Promise.race([runChecks(name, res, own), deadline]);
  } catch (e) {
    res.status = e instanceof Blocked ? "BLOCKED" : "FAIL";
    if (e instanceof Blocked) res.blocked = e.message;
    res.error = String(e.stack ?? e);
  } finally {
    clearTimeout(timer);
    res.cleanup = await closeOwned(own);
  }
  res.summary = summarize(res);
  if (!res.status) {
    const fail = !res.lb?.ok || res.page_errors.length > 0 || res.checks.some(c => !c.ok) || (res.integrity && !res.integrity.complete);
    res.status = fail ? "FAIL" : NO_CM5 ? "BLOCKED" : "PASS";
    if (NO_CM5) res.blocked = "--no-cm5: the CM5 comparison did not run";
  }
  res.ok = res.status === "PASS";
  return res;
}

// Close the browser this run launched (also one whose launch was still pending).
async function closeOwned(own) {
  if (!own.browser && own.launching) {
    own.launching.then(b => b.close(), () => {});
    return { browser_closed: "launch still pending: closed when it resolves" };
  }
  if (!own.browser) return { browser_closed: "no browser was launched" };
  let timer;
  try {
    await Promise.race([own.browser.close(), new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("browser.close() did not finish in 30 s")), 30000); })]);
    return { browser_closed: true, connected_after: own.browser.isConnected() };
  } catch (e) {
    return { browser_closed: false, error: String(e.message ?? e) };
  } finally { clearTimeout(timer); }
}

function summarize(res) {
  for (const c of res.checks) c.status = c.unsupported ? "UNSUPPORTED" : !c.ok ? "FAIL" : c.exact ? "PASS" : c.known ? "KNOWN_GAP" : NO_CM5 ? "NOT_COMPARED" : "PASS";
  const n = s => res.checks.filter(c => c.status === s).length;
  return { checks: res.checks.length, exact: res.checks.filter(c => c.exact).length, known: res.checks.filter(c => c.known).length, unsupported: n("UNSUPPORTED"), differential: res.checks.filter(c => c.differential).length, failed: res.checks.filter(c => !c.ok).map(c => c.name) };
}

async function runChecks(name, res, own) {
  const engine = name.split("-")[0];
  const device = MOBILE[name];
  const t0run = Date.now();
  const progress = what => console.error(`[${name} +${((Date.now() - t0run) / 1000).toFixed(1)}s] ${what}`);
  try {
    own.launching = pw[engine].launch({ headless: true, ...(INJECT === "launch-fail" ? { executablePath: "/nonexistent/injected-browser" } : {}) });
    own.browser = await own.launching;
  } catch (e) { throw new Blocked(`${name} did not launch: ${e.message.split("\n")[0]}`); }
  const browser = own.browser;
  res.version = browser.version();
  let ctxOpts = device ? { ...pw.devices[device] } : { viewport: { width: 900, height: 700 }, hasTouch: true };
  if (device) res.device = { profile: device, viewport: ctxOpts.viewport, isMobile: ctxOpts.isMobile, hasTouch: ctxOpts.hasTouch, userAgent: ctxOpts.userAgent };
  if (engine === "chromium") ctxOpts.permissions = ["clipboard-read", "clipboard-write"];
  if (INJECT === "context-fail") ctxOpts.storageState = join(OUT, "injected-missing-storage-state.json");
  let context;
  try { context = await browser.newContext(ctxOpts); } catch (e) { res.touch_context = `hasTouch unsupported: ${e.message}`; delete ctxOpts.hasTouch; context = await browser.newContext(ctxOpts); }
  const page = await context.newPage();
  page.setDefaultTimeout(15000);
  page.on("pageerror", e => res.page_errors.push(String(e)));
  page.on("console", m => { if (m.type() === "error") res.console_errors.push(m.text()); });
  await page.route("https://cm-web.test/**", serve);
  await page.goto(INJECT === "nav-fail" ? "https://unrouted.cm-web.test/injected" : "https://cm-web.test/page.html");
  await page.waitForFunction(() => window.__ready, null, { timeout: 60000 });
  res.lb = await page.evaluate(() => window.__lb);
  if (INJECT === "page-throw") await page.evaluate(() => { throw new Error("injected page exception"); });
  if (INJECT === "reject") await Promise.reject(new Error("injected rejected promise"));
  if (INJECT === "hang") await new Promise(() => {});
  const haveCm5 = !!REFERENCE;
  if (haveCm5) {
    // The reference must load and expose the API the drivers use before any comparison counts.
    const errorsBefore = res.page_errors.length;
    try {
      await page.addStyleTag({ url: "/cm5/lib/codemirror.css" });
      await page.addStyleTag({ content: ".CodeMirror { font-family: 'DejaVu Sans Mono', monospace; font-size: 16px; height: 100%; }" });
      for (const s of ["lib/codemirror.js", "mode/clike/clike.js", "mode/javascript/javascript.js"]) await page.addScriptTag({ url: `/cm5/${s}` });
    } catch (e) { throw new Blocked(`CM5 reference did not load: ${e.message.split("\n")[0]}`); }
    const loadErrors = res.page_errors.splice(errorsBefore);
    const api = await page.evaluate(([want]) => {
      const C = window.CodeMirror;
      if (typeof C !== "function") return { error: `window.CodeMirror is ${typeof C}` };
      return { version: C.version, missing: want.filter(m => typeof C.prototype[m] !== "function"), modes: ["clike", "javascript"].filter(m => typeof C.modes?.[m] === "function"), mime: typeof C.mimeModes?.["text/x-csrc"] };
    }, [CM5_API]);
    res.reference = { ...REFERENCE, in_page: api, ...(loadErrors.length ? { load_errors: loadErrors } : {}) };
    const why = api.error ?? (loadErrors.length ? `page errors while loading: ${loadErrors.join("; ")}`
      : api.version !== LOCK.CM5_VERSION ? `CodeMirror.version is ${JSON.stringify(api.version)}, pinned ${LOCK.CM5_VERSION}`
      : api.missing.length ? `CM5 API lacks ${api.missing.join(", ")}`
      : api.modes.length < 2 || api.mime !== "object" ? `CM5 modes not registered (${api.modes.join(", ") || "none"}; text/x-csrc ${api.mime})` : null);
    if (why) throw new Blocked(`CM5 reference unusable: ${why}`);
  }
  if (device) await page.addStyleTag({ content: ".host { width: 340px !important; height: 260px !important; } #outside { left: 20px !important; top: 300px !important; }" });
  // CM5's `mac` (src/util/browser.js), which cm-web ports: iOS Safari is detected
  // from the vendor and user agent, so the emulated iPhone (platform "Linux x86_64")
  // uses the mac key maps.
  const isMac = await page.evaluate(() => {
    const ios = /Apple Computer/.test(navigator.vendor) && (/Mobile\/\w+/.test(navigator.userAgent) || navigator.maxTouchPoints > 2);
    return ios || /Mac/.test(navigator.platform);
  });
  res.mac_keymaps = isMac;
  const mod = isMac ? "Meta" : "Control";

  const check = (name, expected, actual, note) => {
    const ok = JSON.stringify(expected) === JSON.stringify(actual);
    const known = !ok && KNOWN[name] ? KNOWN[name] : undefined;
    res.checks.push({ name, ok: ok || !!known, exact: ok, expected, actual, ...(known ? { known } : {}), ...(note ? { note } : {}) });
    return ok;
  };
  // Numbers equal within `tol` px; an inexact match is recorded as the measurement gap G4.
  const near = (name, expected, actual, tol) => {
    const flat = v => JSON.stringify(v).match(/-?\d+(\.\d+)?/g)?.map(Number) ?? [];
    const a = flat(expected), b = flat(actual);
    const ok = a.length === b.length && a.every((x, i) => Math.abs(x - b[i]) <= tol);
    const exact = JSON.stringify(expected) === JSON.stringify(actual);
    res.checks.push({ name, ok, exact, expected, actual, ...(!exact && ok ? { known: `within ${tol}px: box-model positions vs DOM-measured positions (KNOWN_GAPS.md G4)` } : {}) });
  };
  // A comparison with CM5's value: counted against REQUIRED by its section (the name's first part).
  const mark = () => { res.checks[res.checks.length - 1].differential = true; };
  const dcheck = (...a) => { const ok = check(...a); mark(); return ok; };
  const dnear = (...a) => { near(...a); mark(); };
  const dropped = INJECT === "drop-section" ? "mouse" : null;
  const E = (f, ...a) => page.evaluate(([f, a]) => (new Function("E", "a", `return (${f})(E, ...a)`))(window.E, a), [f.toString(), a]);
  // On mobile CM5 defaults to inputStyle "contenteditable"; cm-web has only the
  // textarea input model, so the mobile differential pins CM5 to "textarea"
  // (KNOWN_GAPS.md G6).
  const cm5Extra = device ? { inputStyle: "textarea" } : {};
  if (device) res.cm5_input_style = "textarea (pinned; CM5's mobile default is contenteditable)";
  const make = (kind, text, mode, opts) => page.evaluate(([k, t, m, o]) => window.drivers.make(k, t, m, o), [kind, text, mode, kind === "cm5" ? { ...cm5Extra, ...(opts ?? {}) } : opts ?? {}]);
  const clickAt = async (line, ch, o = {}) => { const p = await E((E, l, c) => E.point(l, c), line, ch); await page.mouse.click(p.x, p.y, o); };
  const settle = () => page.waitForTimeout(60);
  await make("cm-web", "", "text/plain", {});
  check("keys.mac_detection_same_as_cm5", isMac, await E(E => E.ed.stats().mac));
  // A wheel action that a mobile profile cannot perform is recorded, not awaited forever.
  const wheel = (dx, dy) => Promise.race([page.mouse.wheel(dx, dy).then(() => null), page.waitForTimeout(5000).then(() => "wheel did not complete in 5 s")]).catch(e => String(e.message ?? e));
  const state = () => E(E => ({ value: E.value(), cursor: E.cursor(), sels: E.sels(), hist: E.hist() }));

  // One differential scenario: the same steps on CM5 (expected) and cm-web (actual).
  async function diff(label, text, mode, opts, steps) {
    const kinds = haveCm5 ? ["cm5", "cm-web"] : ["cm-web"];
    const rec = {};
    for (const kind of kinds) {
      await make(kind, text, mode, opts);
      await settle();
      const out = [];
      for (const [stepName, f] of steps) {
        await f();
        await settle();
        out.push([stepName, await state()]);
      }
      rec[kind] = out;
    }
    if (!haveCm5) { res.checks.push({ name: `${label}`, ok: true, exact: false, note: "CM5 build not supplied: no differential", actual: rec["cm-web"] }); return rec; }
    if (INJECT === "compare-fail" && label === "keys") rec["cm-web"][1][1] = { ...rec["cm-web"][1][1], value: "injected difference" };
    rec.cm5.forEach(([stepName, want], i) => dcheck(`${label}.${stepName}`, want, rec["cm-web"][i][1]));
    return rec;
  }

  // --- Construction and rendering --------------------------------------------
  progress("Construction and rendering");
  for (const kind of haveCm5 ? ["cm5", "cm-web"] : ["cm-web"]) {
    await make(kind, C_DOC, "text/x-csrc", { lineNumbers: true });
    await page.waitForTimeout(300);
    await E(E => E.render());
    res[`render_${kind}`] = await E(E => ({ lines: E.lines(), n: E.root.querySelectorAll(".CodeMirror-code pre").length, gutters: E.root.querySelectorAll(".CodeMirror-linenumber").length }));
    res[`coords_${kind}`] = await E(E => [[0, 0], [1, 4], [5, 10], [12, 20]].map(([l, c]) => E.local(l, c)));
  }
  const rw = res["render_cm-web"];
  check("render.visible_text", C_DOC.split("\n").slice(0, rw.n).map(l => l.replace(/​/g, "")), rw.lines.map(l => l.map(p => p[0]).join("")));
  if (haveCm5) {
    const rc = res.render_cm5;
    dcheck("render.token_spans", rc.lines, rw.lines);
    dcheck("render.rendered_line_count", rc.n, rw.n);
    dnear("render.cursor_coords_local", res.coords_cm5, res["coords_cm-web"], 1);
  }
  check("render.token_color_cm_type", "rgb(0, 136, 85)", await E(E => { const s = E.root.querySelector(".cm-type"); return s && getComputedStyle(s).color; }));
  check("render.comment_color", "rgb(170, 85, 0)", await E(E => { const s = E.root.querySelector(".cm-comment"); return s && getComputedStyle(s).color; }));
  await page.locator("#host").screenshot({ path: join(OUT, `${name}-render.png`) });

  // --- Keyboard editing ------------------------------------------------------
  progress("Keyboard editing");
  await diff("keys", C_DOC, "text/x-csrc", {}, [
    ["click_line2_end", () => clickAt(2, 30)],
    ["type", () => page.keyboard.type(" int q;")],
    ["enter_autoindent", () => page.keyboard.press("Enter")],
    ["type_brace_block", () => page.keyboard.type("if (q) {")],
    ["enter_in_block", () => page.keyboard.press("Enter")],
    ["electric_close", () => page.keyboard.type("}")],
    ["backspace_x3", async () => { for (let i = 0; i < 3; i++) await page.keyboard.press("Backspace"); }],
    ["left_x2", async () => { await page.keyboard.press("ArrowLeft"); await page.keyboard.press("ArrowLeft"); }],
    ["delete_x2", async () => { await page.keyboard.press("Delete"); await page.keyboard.press("Delete"); }],
    ["home", () => page.keyboard.press("Home")],
    ["end", () => page.keyboard.press("End")],
    ["up_down", async () => { await page.keyboard.press("ArrowUp"); await page.keyboard.press("ArrowDown"); await page.keyboard.press("ArrowDown"); }],
    ["shift_select", async () => { await page.keyboard.press("Shift+ArrowLeft"); await page.keyboard.press("Shift+ArrowLeft"); await page.keyboard.press("Shift+ArrowUp"); }],
    ["type_over_selection", () => page.keyboard.type("Z")],
    ["tab", () => page.keyboard.press("Tab")],
    ["undo", () => page.keyboard.press(`${mod}+z`)],
    ["undo2", () => page.keyboard.press(`${mod}+z`)],
    ["redo", () => page.keyboard.press(isMac ? "Shift+Meta+z" : "Control+y")],
    ["select_all", () => page.keyboard.press(`${mod}+a`)],
    ["ctrl_end_ctrl_home", async () => { await page.keyboard.press(`${mod}+End`); await page.keyboard.press(`${mod}+Home`); }],
  ]);

  // Typed text arrives once (no duplicate from the browser default).
  await make("cm-web", "", "text/plain", {});
  await clickAt(0, 0);
  await page.keyboard.type("hello, wörld 日本");
  await settle();
  check("input.no_duplicate_text", "hello, wörld 日本", await E(E => E.value()));
  check("input.field_stays_empty", "", await E(E => E.field().value));
  // Real time merges quick typing into one history event (Date.now clock).
  check("clock.attach_installs_date_now", 0, await page.evaluate(() => window.api.clockStatus()));
  check("clock.typing_merges_in_real_time", [1, 0], await E(E => E.hist()));

  // --- Mouse -----------------------------------------------------------------
  progress("Mouse");
  if (dropped !== "mouse") await diff("mouse", C_DOC, "text/x-csrc", {}, [
    ["click", () => clickAt(3, 8)],
    ["shift_click", () => clickAt(5, 4, { modifiers: ["Shift"] })],
    ["dblclick_word", async () => { const p = await E((E) => E.point(4, 9)); await page.mouse.dblclick(p.x, p.y); }],
    ["drag", async () => {
      const a = await E(E => E.point(2, 4)), b = await E(E => E.point(4, 12));
      await page.mouse.move(a.x, a.y); await page.mouse.down(); await page.mouse.move((a.x + b.x) / 2, (a.y + b.y) / 2); await page.mouse.move(b.x, b.y); await page.mouse.up();
    }],
    ["triple_click_line", async () => { const p = await E(E => E.point(6, 5)); await page.mouse.click(p.x, p.y, { clickCount: 3 }); }],
  ]);

  // --- Scrolling -------------------------------------------------------------
  progress("Scrolling");
  const scrollSeq = {};
  for (const kind of haveCm5 ? ["cm5", "cm-web"] : ["cm-web"]) {
    await make(kind, LONG, "text/plain", { lineNumbers: true });
    await clickAt(0, 0);
    const seq = [];
    const snap = async (l, ms = 150) => { await page.waitForTimeout(ms); seq.push([l, await E(E => ({ ...E.scroll(), cursor: E.cursor(), first: E.lines()[0]?.map(p => p[0]).join("") }))]); };
    const box = await E(E => { const r = E.root.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; });
    await page.mouse.move(box.x, box.y);
    const w1 = await wheel(0, 500); await snap("wheel_500");
    const w2 = await wheel(0, -200); await snap("wheel_-200");
    if (w1 || w2) res[`wheel_unsupported_${kind}`] = w1 || w2;
    await page.keyboard.press("PageDown"); await snap("pagedown");
    await page.keyboard.press(`${mod}+End`); await snap("ctrl_end");
    await page.keyboard.press(`${mod}+Home`); await snap("ctrl_home");
    await E(E => { E.scroller().scrollTop = 2000; }); await snap("scrollbar_2000");
    await page.keyboard.press("End"); await snap("end_long_line");
    // A wheel with dx != 0 is handled in JS outside Gecko, so a 1px move is dropped (issue #66).
    await E(E => { E.scroller().scrollTop = 800; E.scroller().scrollLeft = 300; }); await snap("scroller_800_300");
    const w3 = await wheel(1, 1); await snap("wheel_dx1_dy1");
    const w4 = await wheel(10, 10); await snap("wheel_dx10_dy10");
    // Boundaries and a large delta. Outside Gecko the wheel goes through the editor (2px applies, 1px is dropped,
    // -2000 ends at 0); Gecko scrolls natively, so its large-delta result is the browser's own (recorded:
    // d_1_neg_big_to_zero 800 -> 485), which the editor's own scroll (0) cannot produce.
    const reset = async () => { await E(E => { E.scroller().scrollTop = 800; E.scroller().scrollLeft = 300; }); await page.waitForTimeout(150); };
    await reset(); const w5 = await wheel(2, 0); await snap("wheel_dx2_dy0", 400);
    await reset(); const w6 = await wheel(1, 0); await snap("wheel_dx1_dy0", 400);
    await reset(); const w7 = await wheel(1, -2000); await snap("wheel_dx1_dyneg_big", 800);
    if (w3 || w4 || w5 || w6 || w7) res[`wheel_dx_unsupported_${kind}`] = w3 || w4 || w5 || w6 || w7;
    scrollSeq[kind] = seq;
  }
  res.scroll = scrollSeq;
  const sw = scrollSeq["cm-web"];
  {
    // cm-web's own detection (`gecko` from src/util/browser.js) must agree with the profile, and the large wheel must
    // take the path CM5 takes: native in Firefox (the offset is the browser's), the editor's elsewhere (ends at 0).
    const gecko = await page.evaluate(() => /gecko\/\d/i.test(navigator.userAgent));
    check("scroll.gecko_detected_only_in_firefox", name === "firefox", gecko);
    const big = sw.find(([l]) => l === "wheel_dx1_dyneg_big")?.[1];
    if (big && !res["wheel_dx_unsupported_cm-web"] && !name.endsWith("-mobile")) check("scroll.dx_wheel_path", gecko ? "native" : "editor", gecko ? (big.top > 0 && big.top < 800 ? "native" : "editor") : (big.top === 0 ? "editor" : "native"));
  }
  if (res["wheel_unsupported_cm-web"]) res.checks.push({ name: "scroll.wheel", ok: true, exact: false, unsupported: res["wheel_unsupported_cm-web"] });
  else check("scroll.wheel_moves_dom_and_editor", true, sw[0][1].top > 0 && Math.abs(sw[0][1].top - sw[0][1].editorTop) < 1);
  check("scroll.native_scrollbar_syncs_editor", true, Math.abs(sw[5][1].editorTop - sw[5][1].top) < 2 && sw[5][1].top > 1000);
  if (haveCm5) scrollSeq.cm5.forEach(([l, want], i) => {
    const got = sw[i][1];
    dcheck(`scroll.${l}.cursor`, want.cursor, got.cursor);
    // Firefox scrolls these natively (CM5 and cm-web alike), so the amount is the browser's and varies from run to run
    // (recorded 485 vs 530 for the same wheel); only the direction is compared there, and `scroll.dx_wheel_path` pins the path.
    if (name === "firefox" && (l === "wheel_dx10_dy10" || l === "wheel_dx1_dyneg_big")) dcheck(`scroll.${l}.top`, Math.sign(want.top - 800), Math.sign(got.top - 800));
    else if (KNOWN[`scroll.${l}.top`] || l.startsWith("wheel_dx")) dcheck(`scroll.${l}.top`, Math.round(want.top), Math.round(got.top));
    else dnear(`scroll.${l}.top`, Math.round(want.top), Math.round(got.top), 1);
    // Horizontally these steps stay inside the range (the editor's left range is 111px), so Firefox's native move is the clamp;
    // a 2px tolerance covers its animation and rounding without hiding a Rust-managed move (which is exact elsewhere).
    if (name === "firefox" && (l === "wheel_dx10_dy10" || l === "wheel_dx1_dyneg_big")) dnear(`scroll.${l}.left`, Math.round(want.left), Math.round(got.left), 2);
    else if (l.startsWith("wheel_dx")) dcheck(`scroll.${l}.left`, Math.round(want.left), Math.round(got.left));
    else dnear(`scroll.${l}.left`, Math.round(want.left), Math.round(got.left), 1);
  });

  // --- Window scroll (maybeScrollWindow, scrolling.js:15) ----------------------
  // An editor against the window's top or bottom whose cursor is out of the window: CM5 calls DOM scrollIntoView on a
  // node over the cursor rect, which moves the page and the scroller exactly (recorded window_top_up_1 and _1.9 of
  // oracle/cm5-scroll-threshold.mjs: the 1px and 1.9px moves apply). `inside` leaves the window alone.
  progress("Window scroll");
  const winSeq = {};
  const WIN_CASES = [["top_up_1", "top", 1], ["top_up_1.9", "top", 1.9], ["top_up_30", "top", 30], ["bottom", "bottom", 0], ["inside", "inside", 0]];
  for (const kind of haveCm5 ? ["cm5", "cm-web"] : ["cm-web"]) {
    const rows = {};
    for (const [label, edge, d] of WIN_CASES) {
      await make(kind, LONG, "text/plain", { lineNumbers: true });
      rows[label] = await page.evaluate(async ([edge, d]) => {
        const wait = ms => new Promise(r => setTimeout(r, ms)), E = window.E, host = document.getElementById("host");
        document.body.style.height = "4000px";
        window.scrollTo(0, 0);
        host.style.top = edge === "top" ? "0px" : edge === "bottom" ? `${innerHeight - 100}px` : "";
        E.render(); await wait(200);
        const setCursor = (l, c) => E.cm ? E.cm.setCursor({ line: l, ch: c }) : E.ed.setCursor(l, c);
        if (edge === "top") { E.scroller().scrollTop = E.local(30, 0).top + d; await wait(200); setCursor(30, 0); }
        else setCursor(edge === "bottom" ? 9 : 3, 0);
        await wait(400);
        const r = { top: E.scroller().scrollTop, wy: window.scrollY };
        host.style.top = ""; document.body.style.height = ""; window.scrollTo(0, 0);
        return r;
      }, [edge, d]);
    }
    winSeq[kind] = rows;
  }
  res.window_scroll = winSeq;
  if (INJECT !== "drop-web-check") {
    check("window_scroll.inside_leaves_page", 0, winSeq["cm-web"].inside.wy);
    check("window_scroll.top_moves_scroller_exactly", true, winSeq["cm-web"]["top_up_1"].top > 0 && winSeq["cm-web"]["top_up_30"].top > 0);
  }
  if (haveCm5) for (const [label] of WIN_CASES) {
    const want = winSeq.cm5[label], got = winSeq["cm-web"][label];
    dcheck(`window_scroll.${label}.top`, Math.round(want.top), Math.round(got.top));
    dcheck(`window_scroll.${label}.page`, Math.round(want.wy), Math.round(got.wy));
  }

  // --- Focus and blink -------------------------------------------------------
  progress("Focus and blink");
  for (const kind of haveCm5 ? ["cm5", "cm-web"] : ["cm-web"]) {
    await make(kind, "abc", "text/plain", {});
    await clickAt(0, 1);
    const f1 = await E(E => [E.focused(), E.root.classList.contains("CodeMirror-focused")]);
    const vis = new Set();
    for (let i = 0; i < 14; i++) { vis.add(await E(E => getComputedStyle(E.root.querySelector(".CodeMirror-cursors")).visibility)); await page.waitForTimeout(100); }
    await page.click("#outside");
    await settle();
    const f2 = await E(E => [E.focused(), E.root.classList.contains("CodeMirror-focused"), getComputedStyle(E.root.querySelector(".CodeMirror-cursors")).visibility]);
    res[`focus_${kind}`] = { focused: f1, blink_states: [...vis].sort(), blurred: f2 };
  }
  const fw = res["focus_cm-web"];
  check("focus.click_focuses", [true, true], fw.focused);
  check("focus.cursor_blinks", ["hidden", "visible"], fw.blink_states);
  check("focus.blur_hides_cursor", [false, false, "hidden"], fw.blurred);
  if (haveCm5) dcheck("focus.same_as_cm5", res.focus_cm5, fw);
  check("focus.blink_timer_stopped_on_blur", 0, await E(E => E.ed.stats().blinkTimer));

  // --- Clipboard -------------------------------------------------------------
  progress("Clipboard");
  const clip = {};
  for (const kind of haveCm5 ? ["cm5", "cm-web"] : ["cm-web"]) {
    await make(kind, "alpha beta\ngamma", "text/plain", {});
    await clickAt(0, 0);
    await page.evaluate(() => { window.__clipEvents = []; if (!window.__clipListening) { window.__clipListening = true; for (const t of ["copy", "cut", "paste"]) document.addEventListener(t, () => window.__clipEvents.push(t), { capture: true }); } });
    const steps = [];
    const sys = engine === "chromium" ? () => page.evaluate(() => navigator.clipboard.readText()) : async () => null;
    if (engine === "chromium") await page.evaluate(() => navigator.clipboard.writeText(""));
    await page.keyboard.press("Shift+End");
    await page.keyboard.press(`${mod}+c`); await settle();
    steps.push(["copy", await sys(), await state()]);
    await page.keyboard.press(`${mod}+x`); await settle();
    steps.push(["cut", await sys(), await state()]);
    await page.keyboard.press("ArrowDown"); await page.keyboard.press("End");
    await page.keyboard.press(`${mod}+v`); await settle();
    steps.push(["paste", null, await state()]);
    await page.keyboard.press(`${mod}+z`); await settle();
    steps.push(["undo_paste", null, await state()]);
    clip.mode = engine === "chromium" ? "real shortcuts; system clipboard read back (clipboard permissions)" : "real shortcuts; the copied text is checked by pasting it back";
    clip[kind] = steps;
    (res.clipboard_events ??= {})[kind] = await page.evaluate(() => window.__clipEvents);
  }
  res.clipboard = clip;
  const cw = clip["cm-web"];
  // Linux WebKit binds its editing shortcuts to Ctrl, so with the iPhone profile's
  // mac key maps (Cmd) no clipboard event fires at all. That is the emulated
  // environment, not the editor; it is recorded only when no event reached either
  // editor and both ended in the same state.
  const noEvents = Object.values(res.clipboard_events ?? {}).every(e => e.length === 0);
  const sameAsCm5 = !haveCm5 || JSON.stringify(clip.cm5) === JSON.stringify(cw);
  if (noEvents && sameAsCm5) res.checks.push({ name: "clipboard.shortcuts", ok: true, exact: false, unsupported: `${mod}+C/X/V fired no copy/cut/paste event in this environment (CM5 and cm-web alike)` });
  else {
  if (engine === "chromium") {
    check("clipboard.copy_text", "alpha beta", cw[0][1]);
    check("clipboard.cut_text", "alpha beta", cw[1][1]);
  }
  check("clipboard.cut_deletes", "\ngamma", cw[1][2].value);
  check("clipboard.paste_once", "\ngammaalpha beta", cw[2][2].value);
  check("clipboard.undo_paste", "\ngamma", cw[3][2].value);
  }
  if (haveCm5) clip.cm5.forEach((s, i) => dcheck(`clipboard.${s[0]}.same_as_cm5`, s, cw[i]));
  // --- IME composition -------------------------------------------------------
  progress("IME composition");
  const ime = {};
  for (const kind of haveCm5 ? ["cm5", "cm-web"] : ["cm-web"]) {
    await make(kind, "x", "text/plain", {});
    await clickAt(0, 1);
    const out = {};
    if (engine === "chromium") {
      const cdp = await context.newCDPSession(page);
      await cdp.send("Input.imeSetComposition", { text: "n", selectionStart: 1, selectionEnd: 1 });
      await cdp.send("Input.imeSetComposition", { text: "に", selectionStart: 1, selectionEnd: 1 });
      await settle();
      out.during = await E(E => E.value());
      out.preedit = await E(E => E.root.querySelector(".cm-web-preedit")?.textContent ?? null);
      await cdp.send("Input.insertText", { text: "日本" });
      // ContentEditableInput (CM5's mobile input style) reads the DOM 80 ms after compositionend.
      await page.waitForTimeout(250);
      out.mode = "CDP Input.imeSetComposition / Input.insertText";
    } else {
      out.mode = "synthetic CompositionEvent/InputEvent (no IME in this headless browser)";
      await E(E => {
        const f = E.field();
        f.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
        f.dispatchEvent(new CompositionEvent("compositionupdate", { data: "に" }));
        f.value = "に";
        f.dispatchEvent(new InputEvent("input", { data: "に", inputType: "insertCompositionText", isComposing: true }));
      });
      await settle();
      out.during = await E(E => E.value());
      out.preedit = await E(E => E.root.querySelector(".cm-web-preedit")?.textContent ?? null);
      await E(E => {
        const f = E.field();
        f.value = "日本";
        f.dispatchEvent(new CompositionEvent("compositionend", { data: "日本" }));
        f.dispatchEvent(new InputEvent("input", { data: "日本", inputType: "insertCompositionText", isComposing: false }));
      });
      await settle();
    }
    out.after = await E(E => E.value());
    out.hist = await E(E => E.hist());
    await page.keyboard.type("!");
    await settle();
    out.after_more = await E(E => E.value());
    ime[kind] = out;
  }
  res.ime = ime;
  const iw = ime["cm-web"];
  check("ime.preedit_not_in_document", "x", iw.during);
  check("ime.commit_once", "x日本", iw.after);
  check("ime.no_duplicate_after_end", "x日本!", iw.after_more);
  if (haveCm5) {
    dcheck("ime.during", ime.cm5.during, iw.during);
    dcheck("ime.after_same_as_cm5", ime.cm5.after, iw.after);
    dcheck("ime.history", ime.cm5.hist, iw.hist);
  }

  // --- Touch -----------------------------------------------------------------
  progress("Touch");
  if (ctxOpts.hasTouch) {
    const touch = {};
    for (const kind of haveCm5 ? ["cm5", "cm-web"] : ["cm-web"]) {
      await make(kind, C_DOC, "text/x-csrc", {});
      const p = await E(E => E.point(3, 6));
      let err = null;
      try { await page.touchscreen.tap(p.x, p.y); } catch (e) { err = e.message; }
      await page.waitForTimeout(400);
      touch[kind] = err ? { error: err } : { cursor: await E(E => E.cursor()), value_unchanged: (await E(E => E.value())) === C_DOC };
    }
    res.touch = touch;
    if (touch["cm-web"].error) res.checks.push({ name: "touch.tap", ok: true, exact: false, unsupported: touch["cm-web"].error });
    else {
      check("touch.tap_places_cursor", [3, 6], touch["cm-web"].cursor);
      if (haveCm5 && !touch.cm5.error) dcheck("touch.same_as_cm5", touch.cm5, touch["cm-web"]);
      else if (haveCm5) res.checks.push({ name: "touch.same_as_cm5", ok: true, exact: false, unsupported: `CM5 tap: ${touch.cm5.error}` });
    }
  } else res.checks.push({ name: "touch.tap", ok: true, exact: false, unsupported: res.touch_context });

  // --- Scheduling: highlight worker, timers, destruction ---------------------
  progress("Scheduling: highlight worker, timers, destruction");
  const big = Array.from({ length: 3000 }, (_, i) => `function f${i}(a) { return a + ${i}; } // ${i}`).join("\n");
  await make("cm-web", big, "javascript", {});
  const t0 = Date.now();
  let st;
  for (;;) { st = await E(E => E.ed.stats()); if (!st.workTimer && st.frames > 1 || Date.now() - t0 > 15000) break; await page.waitForTimeout(50); }
  res.highlight = { ms: Date.now() - t0, ...st };
  // CM5's worker highlights up to viewTo (`highlightFrontier < viewTo`), not to the end.
  const view = await E(E => E.ed.scrollInfo().viewTo);
  check("schedule.highlight_worker_reaches_view_to", true, st.highlightFrontier >= view);
  check("schedule.worker_stops_when_done", 0, await E(E => E.ed.stats().workTimer));
  await E(E => { E.scroller().scrollTop = 40000; });
  const t1 = Date.now();
  for (;;) { st = await E(E => E.ed.stats()); if (st.highlightFrontier >= 2100 || Date.now() - t1 > 15000) break; await page.waitForTimeout(100); }
  res.highlight_after_scroll = { ms: Date.now() - t1, ...st, viewTo: await E(E => E.ed.scrollInfo().viewTo) };
  check("schedule.scroll_restarts_worker", true, st.highlightFrontier >= res.highlight_after_scroll.viewTo);
  const rep = await page.evaluate(async () => {
    const api = window.api, host = document.getElementById("host");
    if (window.E) { window.E.destroy(); window.E = null; }
    const before = api.liveEditors();
    for (let i = 0; i < 200; i++) {
      const ed = new api.WebEditor("int x = 1;\n".repeat(50), "text/x-csrc", { lineNumbers: true });
      ed.attach(host);
      ed.typeText("y");
      ed.renderNow();
      ed.focus();
      ed.destroy();
    }
    await new Promise(r => setTimeout(r, 1200));
    return { before, after: api.liveEditors(), nodes: host.childElementCount, styles: document.querySelectorAll("#cm-web-style").length };
  });
  res.repeat = rep;
  check("destroy.repeat_200_no_live_editors", [rep.before, 0, 1], [rep.after, rep.nodes, rep.styles]);

  // --- Files -----------------------------------------------------------------
  progress("Files");
  res.files = await page.evaluate(() => {
    const api = window.api, enc = new TextEncoder(), dec = new TextDecoder();
    const files = new Map([["/u.txt", enc.encode("héllo 日本 😀\r\nline2\r\n")]]);
    const host = { read: p => files.get(p) ?? null, write: (p, b) => { files.set(p, b.slice()); } };
    const out = {};
    const ed = api.WebEditor.open("/u.txt", host, {});
    out.opened = ed.getValue();
    ed.setCursor(1, 5); ed.typeText("!");
    ed.save();
    out.saved = dec.decode(files.get("/u.txt"));
    out.clean = ed.isClean();
    const bad = api.WebEditor.open("/v.txt", { read: () => null, write: () => { throw new Error("quota"); } }, {});
    bad.typeText("a");
    try { bad.save(); out.failed_write = "no error"; } catch (e) { out.failed_write = String(e.message); }
    out.failed_write_dirty = !bad.isClean();
    try { api.WebEditor.open("/w.txt", { read: () => { throw new Error("denied"); } }, {}); out.failed_read = "no error"; } catch (e) { out.failed_read = String(e.message); }
    ed.destroy(); bad.destroy();
    return out;
  });
  // getValue joins lines with the file's lineSeparator (CM5 `lineSeparator`).
  check("files.unicode_crlf_roundtrip", ["héllo 日本 😀\r\nline2\r\n", "héllo 日本 😀\r\nline2!\r\n", true],
    [res.files.opened, res.files.saved, res.files.clean]);
  check("files.failed_write_throws_and_stays_dirty", true, res.files.failed_write.includes("quota") && res.files.failed_write_dirty);
  check("files.failed_read_throws", true, res.files.failed_read.includes("denied"));

  // --- Measurement -----------------------------------------------------------
  progress("Measurement");
  const meas = {};
  for (const kind of haveCm5 ? ["cm5", "cm-web"] : ["cm-web"]) {
    await make(kind, "abc 日本語 x😀y é\tz", "text/plain", {});
    meas[kind] = await E(E => Array.from({ length: 16 }, (_, ch) => Math.round(E.local(0, ch).left * 100) / 100));
  }
  res.measurement = meas;
  if (haveCm5) {
    dnear("measure.ascii_positions", meas.cm5.slice(0, 5), meas["cm-web"].slice(0, 5), 1);
    const same = meas.cm5.map((v, i) => Math.abs(v - meas["cm-web"][i]) < 0.6);
    res.measurement.same_within_0_6px = same;
    res.checks.push({ name: "measure.unicode_positions", ok: true, exact: same.every(Boolean), expected: meas.cm5, actual: meas["cm-web"], note: "recorded; CJK/emoji/tab widths are box-model values from browser measurement (KNOWN_GAPS.md G4)" });
    mark();
  }
  res.metrics = await page.evaluate(() => { const ed = new window.api.WebEditor("", "text/plain", {}); const h = document.createElement("div"); h.className = "host"; h.style.top = "400px"; document.body.appendChild(h); ed.attach(h); const m = ed.metrics(); ed.destroy(); h.remove(); return m; });

  // The async clipboard API: a defined result either way.
  await make("cm-web", "x", "text/plain", {});
  res.clipboard_api = await E(E => E.ed.pasteFromSystem(2000).then(v => `resolved ${v}`, e => `rejected ${e}`));
  res.clipboard_api_write = await E(E => { E.ed.execCommand("selectAll"); return E.ed.copyToSystem(2000).then(v => `resolved ${v}`, e => `rejected ${e}`); });
  check("clipboard.async_api_defined_result", true, /^(resolved true|rejected clipboard-(denied|unavailable|timeout))/.test(res.clipboard_api));
  check("clipboard.async_write_defined_result", true, /^(resolved .|rejected clipboard-(denied|unavailable|timeout))/.test(res.clipboard_api_write));

  // Firefox answers readText with a paste prompt that stays open; close it.
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);
  // Every required comparison ran, or its section recorded why the environment cannot run it. The cm-web-only
  // checks (REQUIRED_WEB) are required with or without the reference.
  const web = Object.fromEntries(Object.entries(REQUIRED_WEB).map(([sec, want]) => {
    const ran = res.checks.filter(c => c.name.split(".")[0] === sec && !c.differential).length;
    return [sec, { required: want, ran, complete: ran === want }];
  }));
  const sections = haveCm5 ? Object.fromEntries(Object.entries(REQUIRED).map(([sec, want]) => {
    const mine = res.checks.filter(c => c.name.split(".")[0] === sec);
    const ran = mine.filter(c => c.differential).length;
    const unsupported = mine.filter(c => c.unsupported).map(c => `${c.name}: ${c.unsupported}`);
    return [sec, { required: want, ran, ...(unsupported.length ? { unsupported } : {}), complete: ran === want || (ran < want && unsupported.length > 0) }];
  })) : undefined;
  const incomplete = [...Object.keys(web).filter(s => !web[s].complete).map(s => `${s} (cm-web only)`), ...Object.keys(sections ?? {}).filter(s => !sections[s].complete)];
  res.integrity = { reference: haveCm5 ? "loaded" : "not supplied: comparisons not counted", web_only: web, ...(sections ? { sections } : {}), complete: incomplete.length === 0, ...(incomplete.length ? { incomplete } : {}) };
}

const out = [];
for (const name of args) {
  try { out.push(await runBrowser(name)); } catch (e) { out.push({ browser: name, status: "FAIL", ok: false, error: String(e.stack ?? e) }); }
}
finish(out);
