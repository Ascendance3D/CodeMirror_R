// CM5_PLAYWRIGHT_FROM=<dir with node_modules/playwright>/ \
//   node metrics-lifecycle.mjs --pkg <wasm-bindgen pkg dir> --cm5 <built cm5 dir> [--out <dir>] <browser>...
//
// The adapter's font-measurement lifecycle, compared with the pinned CodeMirror 5 in
// the same page: an editor created hidden, shown, hidden again, resized (container
// and window), refreshed after a font change, and created with a 0 or a small
// (0 < w <= 2) measured character width. CM5's values are the expected values for
// what CM5 has (charWidth through cursor positions and the content width,
// `defaultCharWidth`, `refresh()`); the adapter-only checks (listeners, observers,
// measurements, cleanup) are port acceptance checks, counted from instrumented
// `addEventListener`/`ResizeObserver` and the adapter's `cm-web-measure` element.
// Separate from browser-run.mjs, whose 60 required comparisons it does not change.
//
// CM5 rules followed (src/measurement/position_measurement.js:587-615,
// src/edit/global_events.js:28-44, src/edit/methods.js:411-421): `charWidth` returns
// `width || 10` and caches only `width > 2`; `textHeight` caches only `height > 3`;
// an uncached value is measured again on the next call; `refresh()` and the
// (100 ms debounced) window `resize` clear both caches.
//
// Exit status: 0 when every check of every browser passed, 1 otherwise, 2 when the
// reference or Playwright is unavailable. Development-only Node use.
import { createRequire } from "node:module";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const opt = k => { const i = args.indexOf(k); if (i < 0) return null; const v = args[i + 1]; args.splice(i, 2); return v; };
const PKG = resolve(opt("--pkg") ?? "");
const CM5 = opt("--cm5");
const OUT = resolve(opt("--out") ?? ".");
mkdirSync(OUT, { recursive: true });
const unavailable = why => { console.log(`UNAVAILABLE: ${why}`); process.exit(2); };
if (!process.env.CM5_PLAYWRIGHT_FROM) unavailable("CM5_PLAYWRIGHT_FROM is not set (Playwright)");
if (!CM5 || !readFileSync(join(CM5, "lib/codemirror.js"), "utf8").includes('CodeMirror.version = "5.65.21"')) unavailable("--cm5 must name a 5.65.21 build");
if (!existsSync(join(PKG, "cm_web_bg.wasm"))) unavailable(`--pkg ${PKG} has no cm_web_bg.wasm`);
const pw = createRequire(process.env.CM5_PLAYWRIGHT_FROM)("playwright");
const MOBILE = { "chromium-mobile": "Pixel 7", "webkit-mobile": "iPhone 15" };

const TYPES = { ".mjs": "text/javascript", ".js": "text/javascript", ".html": "text/html", ".css": "text/css", ".wasm": "application/wasm" };
function serve(route) {
  const path = new URL(route.request().url()).pathname;
  const file = path.startsWith("/pkg/") ? join(PKG, path.slice(5)) : path.startsWith("/cm5/") ? join(CM5, path.slice(5)) : join(HERE, path.slice(1));
  if (!existsSync(file)) return route.fulfill({ status: 404, body: "not found" });
  return route.fulfill({ body: readFileSync(file), contentType: TYPES[extname(file)] ?? "application/octet-stream" });
}

// Counts window listeners, ResizeObservers and adapter measurements; runs before page.mjs.
function instrument() {
  const I = window.__inst = { add: {}, remove: {}, ro: { created: 0, observe: 0, disconnect: 0 }, measures: 0 };
  const add = EventTarget.prototype.addEventListener, rem = EventTarget.prototype.removeEventListener;
  EventTarget.prototype.addEventListener = function (t, f, o) { if (this === window) I.add[t] = (I.add[t] ?? 0) + 1; return add.call(this, t, f, o); };
  EventTarget.prototype.removeEventListener = function (t, f, o) { if (this === window) I.remove[t] = (I.remove[t] ?? 0) + 1; return rem.call(this, t, f, o); };
  const RO = window.ResizeObserver;
  window.ResizeObserver = class extends RO {
    constructor(cb) { super(cb); I.ro.created++; }
    observe(...a) { I.ro.observe++; return super.observe(...a); }
    disconnect() { I.ro.disconnect++; return super.disconnect(); }
  };
  new MutationObserver(ms => { for (const m of ms) for (const n of m.addedNodes) if (n.classList?.contains("cm-web-measure")) I.measures++; })
    .observe(document, { childList: true, subtree: true });
}

// The page side: hosts and one driver shape for both editors.
function setup() {
  const api = window.api;
  window.L = {
    host(style = "", cls = "") {
      const h = document.createElement("div");
      h.className = cls;
      h.style.cssText = `position:absolute;left:20px;top:360px;width:600px;height:200px;${style}`;
      document.body.appendChild(h);
      return h;
    },
    make(kind, text, host) {
      if (kind === "cm5") {
        const cm = window.CodeMirror(host, { value: text, mode: "text/plain" });
        const s = () => cm.getScrollInfo();
        return {
          root: cm.getWrapperElement(), cm,
          cw: () => cm.defaultCharWidth(), cached: () => cm.display.cachedCharWidth,
          th: () => cm.defaultTextHeight(),
          x: (l, c) => cm.cursorCoords({ line: l, ch: c }, "local").left,
          width: () => s().width, clientWidth: () => s().clientWidth,
          state: () => ({ value: cm.getValue(), sels: cm.listSelections().map(r => [r.anchor.line, r.anchor.ch, r.head.line, r.head.ch]), hist: [cm.historySize().undo, cm.historySize().redo] }),
          select: (a, b, c, d) => cm.setSelection({ line: a, ch: b }, { line: c, ch: d }),
          type: t => cm.replaceSelection(t),
          refresh: () => { cm.refresh(); return true; },
          destroy: () => cm.getWrapperElement().remove(),
        };
      }
      const ed = new api.WebEditor(text, "text/plain", {});
      ed.attach(host);
      ed.renderNow();
      return {
        root: host.querySelector(".CodeMirror"), ed,
        cw: () => ed.metrics().charWidth, th: () => ed.metrics().textHeight,
        x: (l, c) => ed.cursorCoords(l, c).left,
        width: () => ed.scrollInfo().width, clientWidth: () => ed.scrollInfo().clientWidth,
        state: () => ({ value: ed.getValue(), sels: Array.from(ed.listSelections(), r => Array.from(r)), hist: Array.from(ed.historySize()) }),
        select: (a, b, c, d) => ed.setSelection(a, b, c, d),
        type: t => ed.typeText(t),
        refresh: () => typeof ed.refresh === "function" ? (ed.refresh(), true) : false,
        destroy: () => ed.destroy(),
      };
    },
    // Window listeners and observers active now (adds minus removes).
    counts() {
      const I = window.__inst, live = t => (I.add[t] ?? 0) - (I.remove[t] ?? 0);
      return { resize: live("resize"), mousemove: live("mousemove"), mouseup: live("mouseup"), ro: I.ro.created - I.ro.disconnect, measures: I.measures, live: api.liveEditors() };
    },
  };
  return true;
}

// The longest line is wider than the 600px editor, so the content width follows it.
const TEXT = `abcdefghij klmnop\nshort\nthe longest line ${"x".repeat(80)}\nend`;

async function runBrowser(name) {
  const engine = name.split("-")[0], device = MOBILE[name];
  const res = { browser: name, checks: [], page_errors: [] };
  const browser = await pw[engine].launch({ headless: true });
  try {
    res.version = browser.version();
    const context = await browser.newContext(device ? { ...pw.devices[device] } : { viewport: { width: 900, height: 700 } });
    if (device) res.device = device;
    await context.addInitScript(instrument);
    const page = await context.newPage();
    page.setDefaultTimeout(15000);
    page.on("pageerror", e => res.page_errors.push(String(e)));
    await page.route("https://cm-web.test/**", serve);
    await page.goto("https://cm-web.test/page.html");
    await page.waitForFunction(() => window.__ready, null, { timeout: 60000 });
    await page.addStyleTag({ url: "/cm5/lib/codemirror.css" });
    await page.addStyleTag({ content: ".CodeMirror { font-family: 'DejaVu Sans Mono', monospace; font-size: 16px; height: 100%; }" });
    await page.addScriptTag({ url: "/cm5/lib/codemirror.js" });
    res.cm5_version = await page.evaluate(() => window.CodeMirror.version);
    await page.evaluate(setup);
    await page.evaluate(() => { document.getElementById("host").textContent = ""; });
    res.refresh_api = await page.evaluate(() => typeof window.api.WebEditor.prototype.refresh === "function");

    const P = (f, ...a) => page.evaluate(([f, a]) => (new Function("a", `return (${f})(...a)`))(a), [f.toString(), a]);
    // Two animation frames and a margin: ResizeObserver callbacks and the frame they schedule.
    const frames = () => page.evaluate(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(r, 30)))));
    const record = (name, ok, expected, actual, extra = {}) => res.checks.push({ name, ok, expected, actual, ...extra });
    const near = (name, expected, actual, tol, extra) => record(name, typeof expected === "number" && typeof actual === "number" && Math.abs(expected - actual) <= tol, expected, actual, { tol, ...extra });
    const same = (name, expected, actual, extra) => record(name, JSON.stringify(expected) === JSON.stringify(actual), expected, actual, extra);
    const cm5 = { source: "CM5" }, port = { source: "port acceptance" };
    // Both editors, same text, own hosts; returns snapshot functions by kind.
    const pair = async (style = "", cls = "") => {
      await P((s, c) => { window.A = window.L.make("cm5", window.TEXT, window.L.host(s, c)); window.B = window.L.make("cm-web", window.TEXT, window.L.host(s, c)); window.HA = window.A.root.parentNode; window.HB = window.B.root.parentNode; }, style, cls);
      await frames();
    };
    await page.evaluate(t => { window.TEXT = t; }, TEXT);
    const snap = () => P(() => {
      const s = E => ({ cw: E.cw(), x10: E.x(0, 10), width: E.width(), clientWidth: E.clientWidth(), state: E.state() });
      return { cm5: { ...s(window.A), cached: window.A.cached() }, web: s(window.B) };
    });
    const both = f => P(f => { const g = new Function("E", "H", `return (${f})(E, H)`); return [g(window.A, window.HA), g(window.B, window.HB)]; }, f.toString());
    const drop = () => P(() => { window.A.destroy(); window.HA.remove(); window.B.destroy(); window.HB.remove(); });
    // CM5 keeps line measurements and its sizer width until refresh(): a hidden
    // editor shown without refresh() keeps a 0 sizer, and onResize clears only the
    // character-width caches. The adapter lays lines out from its metrics, so it
    // shows the current geometry at once. When CM5's value differs, CM5 is refreshed
    // and the adapter must equal the refreshed value; that is recorded as KNOWN.
    const STALE = "CM5 keeps its line measurements and sizer width until refresh() (position_measurement.js:278-283; global_events.js:38-44 clears only cachedCharWidth/TextHeight/PaddingH); compared with CM5 after refresh()";
    const compare = async (label, s) => {
      near(`${label}.char_width`, s.cm5.cw, s.web.cw, 0.01, cm5);
      const fits = r => Math.abs(r.x10 - s.web.x10) <= 1 && Math.abs(r.width - s.web.width) <= 2;
      if (fits(s.cm5)) {
        near(`${label}.cursor_x_ch10`, s.cm5.x10, s.web.x10, 1, cm5);
        near(`${label}.content_width`, s.cm5.width, s.web.width, 2, cm5);
        return;
      }
      const r = await P(() => { window.A.refresh(); return { x10: window.A.x(0, 10), width: window.A.width() }; });
      for (const [k, tol] of [["x10", 1], ["width", 2]]) {
        const name = `${label}.${k === "x10" ? "cursor_x_ch10" : "content_width"}`;
        if (Math.abs(s.cm5[k] - s.web[k]) <= tol) near(name, s.cm5[k], s.web[k], tol, cm5);
        else record(name, Math.abs(r[k] - s.web[k]) <= tol, r[k], s.web[k], { tol, known: STALE, cm5_before_refresh: s.cm5[k], source: "CM5 after refresh()" });
      }
    };
    // 1. Visible at creation.
    await pair();
    let s = await snap();
    res.visible = s;
    await compare("visible", s);
    await drop();
    // CM5 registers its window handlers once, with its first editor.
    const base = await P(() => window.L.counts());
    res.base_counts = base;

    // 2-3. Hidden at creation (display: none), then shown without refresh().
    await pair("display:none");
    await both((E, H) => E.select(0, 2, 0, 5));
    s = await snap();
    res.hidden_at_attach = s;
    same("hidden.cm5_char_width_fallback", 10, s.cm5.cw, { source: "CM5 (width || 10, not cached)" });
    same("hidden.cm5_not_cached", null, s.cm5.cached, cm5);
    same("hidden.adapter_reports_the_raw_measurement", 0, s.web.cw, { source: "port contract: the DOM measures 0; cm-view applies width || 10" });
    const before = s.web.state;
    await both((E, H) => { H.style.display = ""; });
    await frames();
    s = await snap();
    res.hidden_then_shown = s;
    await compare("shown", s);
    same("shown.state_unchanged", before, s.web.state, port);
    same("shown.state_same_as_cm5", s.cm5.state, s.web.state, cm5);

    // 4. Visible, then hidden (the cache keeps the valid width), shown again; then
    // hidden + refresh() (measures 0) and shown without refresh().
    await both((E, H) => { H.style.display = "none"; });
    await frames();
    s = await snap();
    res.visible_then_hidden = s;
    same("hide.cm5_keeps_cached_width", true, s.cm5.cached > 2, cm5);
    near("hide.char_width_kept", s.cm5.cw, s.web.cw, 0.01, cm5);
    await both((E, H) => { H.style.display = ""; });
    await frames();
    s = await snap();
    await compare("hide_show", s);
    await both((E, H) => { H.style.display = "none"; E.refresh(); });
    await frames();
    s = await snap();
    res.hidden_refresh = s;
    same("hidden_refresh.cm5_char_width", 10, s.cm5.cw, cm5);
    same("hidden_refresh.adapter_raw_measurement", res.refresh_api ? 0 : "refresh() absent", res.refresh_api ? s.web.cw : "refresh() absent", port);
    await both((E, H) => { H.style.display = ""; });
    await frames();
    s = await snap();
    await compare("hidden_refresh_show", s);
    same("hidden_refresh_show.state_unchanged", before, s.web.state, port);

    // 5. Container resize: the size follows, the cached width stays (CM5 setSize).
    const m0 = (await P(() => window.L.counts())).measures;
    for (const w of [300, 450, 600]) {
      await both(new Function("E", "H", `H.style.width = "${w}px";`));
      await frames();
      s = await snap();
      near(`container_${w}.client_width`, s.cm5.clientWidth, s.web.clientWidth, 1, cm5);
      near(`container_${w}.char_width`, s.cm5.cw, s.web.cw, 0.01, cm5);
    }
    res.container_resize_measures = (await P(() => window.L.counts())).measures - m0;
    same("container_resize.no_measurement_while_cached", 0, res.container_resize_measures, port);

    // 6. Font change, then refresh(); before refresh() both keep the cached width.
    await both((E, H) => { E.root.style.fontSize = "12px"; });
    await frames();
    s = await snap();
    res.font_change_no_refresh = s;
    near("font_no_refresh.both_keep_cached_width", s.cm5.cw, s.web.cw, 0.01, cm5);
    await both((E, H) => E.refresh());
    await frames();
    s = await snap();
    res.font_change_refresh = s;
    await compare("font_refresh", s);
    same("font_refresh.state_unchanged", before, s.web.state, port);

    // 7. Window resize after a text-size change: CM5's onResize clears the caches.
    await both((E, H) => { E.root.style.fontSize = "20px"; });
    const vp = page.viewportSize();
    await page.setViewportSize({ width: vp.width - 40, height: vp.height });
    await page.waitForTimeout(250);
    await frames();
    s = await snap();
    res.window_resize = s;
    await compare("window_resize", s);
    await page.setViewportSize(vp);
    await page.waitForTimeout(250);

    // 8. Repeated resize and hide/show: no new listener or observer.
    const c1 = await P(() => window.L.counts());
    for (let i = 0; i < 10; i++) {
      await page.setViewportSize({ width: vp.width - 10 * (i % 2), height: vp.height });
      await both(new Function("E", "H", `H.style.width = "${500 + 10 * i}px"; H.style.display = "${i % 2 ? "" : "none"}";`));
      await frames();
    }
    await both((E, H) => { H.style.display = ""; H.style.width = "600px"; });
    await page.setViewportSize(vp);
    await page.waitForTimeout(250);
    await frames();
    const c2 = await P(() => window.L.counts());
    res.repeat = { before: c1, after: c2 };
    same("repeat.window_listeners_unchanged", [c1.resize, c1.mousemove, c1.mouseup, c1.ro], [c2.resize, c2.mousemove, c2.mouseup, c2.ro], port);
    s = await snap();
    await compare("repeat", s);
    same("repeat.state_unchanged", before, s.web.state, port);

    // 9. Idle: no measurement without a change.
    const i0 = (await P(() => window.L.counts())).measures;
    await page.waitForTimeout(1000);
    res.idle_measures = (await P(() => window.L.counts())).measures - i0;
    same("idle.no_measurement", 0, res.idle_measures, port);
    await drop();

    // 10-11. A 0 width (font-size 0) and a small real width (font-size 2px) at creation.
    await page.addStyleTag({ content: ".lc-fs0 .CodeMirror { font-size: 0px } .lc-fs2 .CodeMirror { font-size: 2px }" });
    for (const [label, cls] of [["zero", "lc-fs0"], ["small", "lc-fs2"]]) {
      await pair("", cls);
      s = await snap();
      res[`${label}_width`] = s;
      if (label === "zero") {
        same("zero.cm5_char_width_fallback", 10, s.cm5.cw, { source: "CM5 (width || 10)" });
        same("zero.adapter_raw_measurement", 0, s.web.cw, { source: "port contract: the DOM measures 0; cm-view applies width || 10" });
      } else {
        same("small.cm5_width_in_(0,2]", true, s.cm5.cw > 0 && s.cm5.cw <= 2, cm5);
        same("small.cm5_not_cached", null, s.cm5.cached, cm5);
        await compare("small", s);
      }
      // Uncached: the next use measures again, with no refresh().
      await both((E, H) => { H.className = ""; E.type("x"); });
      await frames();
      s = await snap();
      await compare(`${label}_recover`, s);
      same(`${label}_recover.state_same_as_cm5`, s.cm5.state, s.web.state, cm5);
      await drop();
    }

    // 12. Two editors: hiding and showing one leaves the other alone.
    const two = await P(() => {
      const a = window.L.make("cm-web", "one", window.L.host("top:360px")), b = window.L.make("cm-web", "two", window.L.host("top:580px;font-size:12px"));
      b.root.style.fontSize = "12px"; b.refresh();
      return { a: a.cw(), b: b.cw(), counts: window.L.counts(), keep: (window.TWO = [a, b]) && true };
    });
    await P(() => { window.TWO[1].root.parentNode.style.display = "none"; });
    await frames();
    await P(() => { window.TWO[1].root.parentNode.style.display = ""; });
    await frames();
    const two2 = await P(() => ({ a: window.TWO[0].cw(), b: window.TWO[1].cw() }));
    res.two_editors = { before: two, after: two2 };
    same("two_editors.own_widths", [two.a, two.b], [two2.a, two2.b], port);
    same("two_editors.different_fonts", true, two.a > two2.b, port);
    same("two_editors.window_resize_listeners", base.resize + 2, two.counts.resize, { source: "port acceptance: one per attached editor" });

    // 13. Detach: every listener and observer is gone, a later resize is harmless.
    await P(() => { for (const e of window.TWO) { const h = e.root.parentNode; e.destroy(); h.remove(); } });
    await page.setViewportSize({ width: vp.width - 30, height: vp.height });
    await page.waitForTimeout(250);
    await page.setViewportSize(vp);
    await page.waitForTimeout(250);
    const end = await P(() => window.L.counts());
    res.end_counts = end;
    same("detach.no_listeners_or_observers", [base.resize, base.mousemove, base.mouseup, base.ro, 0], [end.resize, end.mousemove, end.mouseup, end.ro, end.live], port);
    same("detach.no_page_errors", [], res.page_errors, port);
  } finally {
    await browser.close();
  }
  res.failed = res.checks.filter(c => !c.ok).map(c => c.name);
  res.summary = { checks: res.checks.length, exact: res.checks.filter(c => c.ok && !c.known).length, known: res.checks.filter(c => c.ok && c.known).length, failed: res.failed.length };
  res.status = res.failed.length === 0 && res.page_errors.length === 0 ? "PASS" : "FAIL";
  return res;
}

const out = [];
for (const name of args) {
  try { out.push(await runBrowser(name)); } catch (e) { out.push({ browser: name, status: "FAIL", error: String(e.stack ?? e) }); }
}
writeFileSync(join(OUT, "metrics-lifecycle.json"), JSON.stringify(out, null, 1));
for (const r of out) console.log(`${r.browser} ${r.status} ${r.summary ? `${r.checks.length - r.summary.failed}/${r.checks.length} (known ${r.summary.known})` : ""}${r.failed?.length ? ` failed: ${r.failed.join(", ")}` : ""}${r.error ? ` ${r.error.split("\n")[0]}` : ""}`);
process.exitCode = out.every(r => r.status === "PASS") ? 0 : 1;
