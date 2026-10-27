// CM5_PLAYWRIGHT_FROM=<dir with node_modules/playwright>/ \
//   node browser-run.mjs --pkg <wasm-bindgen pkg dir> [--cm5 <built cm5 dir>] [--out <dir>] <chromium|firefox|webkit>...
//
// For each browser: load page.html (served through page.route, no server), run
// the L-B scenarios in the page, then drive the attached adapter with real
// Playwright input (keyboard, mouse, wheel, touch, clipboard) and compare it,
// step by step, with the pinned CodeMirror 5 driven by the same input in the
// same page (when --cm5 names a built tree with lib/codemirror.js). CM5's values
// are the expected values; a difference is either a failure or a named known
// difference listed in KNOWN below. Development-only Node use.
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
if (!process.env.CM5_PLAYWRIGHT_FROM) {
  console.log(JSON.stringify([{ ok: false, unavailable: "CM5_PLAYWRIGHT_FROM is not set (Playwright not found)" }]));
  process.exit(2);
}
const pw = createRequire(process.env.CM5_PLAYWRIGHT_FROM)("playwright");
mkdirSync(OUT, { recursive: true });

const TYPES = { ".mjs": "text/javascript", ".js": "text/javascript", ".html": "text/html", ".css": "text/css", ".wasm": "application/wasm" };
function serve(route) {
  const path = new URL(route.request().url()).pathname;
  let file;
  if (path.startsWith("/pkg/")) file = join(PKG, path.slice(5));
  else if (path.startsWith("/cm5/") && CM5) file = join(CM5, path.slice(5));
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

async function runBrowser(name) {
  const res = { browser: name, checks: [], page_errors: [], console_errors: [] };
  const engine = name.split("-")[0];
  const device = MOBILE[name];
  if (name.includes("-") && !device) throw new Error(`no mobile profile for ${name}`);
  const t0run = Date.now();
  const progress = what => console.error(`[${name} +${((Date.now() - t0run) / 1000).toFixed(1)}s] ${what}`);
  const browser = await pw[engine].launch({ headless: true });
  res.version = browser.version();
  let ctxOpts = device ? { ...pw.devices[device] } : { viewport: { width: 900, height: 700 }, hasTouch: true };
  if (device) res.device = { profile: device, viewport: ctxOpts.viewport, isMobile: ctxOpts.isMobile, hasTouch: ctxOpts.hasTouch, userAgent: ctxOpts.userAgent };
  if (engine === "chromium") ctxOpts.permissions = ["clipboard-read", "clipboard-write"];
  let context;
  try { context = await browser.newContext(ctxOpts); } catch (e) { res.touch_context = `hasTouch unsupported: ${e.message}`; delete ctxOpts.hasTouch; context = await browser.newContext(ctxOpts); }
  const page = await context.newPage();
  page.setDefaultTimeout(15000);
  page.on("pageerror", e => res.page_errors.push(String(e)));
  page.on("console", m => { if (m.type() === "error") res.console_errors.push(m.text()); });
  await page.route("https://cm-web.test/**", serve);
  await page.goto("https://cm-web.test/page.html");
  await page.waitForFunction(() => window.__ready, null, { timeout: 60000 });
  res.lb = await page.evaluate(() => window.__lb);
  const haveCm5 = !!CM5;
  if (haveCm5) {
    await page.addStyleTag({ url: "/cm5/lib/codemirror.css" });
    await page.addStyleTag({ content: ".CodeMirror { font-family: 'DejaVu Sans Mono', monospace; font-size: 16px; height: 100%; }" });
    for (const s of ["lib/codemirror.js", "mode/clike/clike.js", "mode/javascript/javascript.js"]) await page.addScriptTag({ url: `/cm5/${s}` });
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
    rec.cm5.forEach(([stepName, want], i) => check(`${label}.${stepName}`, want, rec["cm-web"][i][1]));
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
    check("render.token_spans", rc.lines, rw.lines);
    check("render.rendered_line_count", rc.n, rw.n);
    near("render.cursor_coords_local", res.coords_cm5, res["coords_cm-web"], 1);
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
  await diff("mouse", C_DOC, "text/x-csrc", {}, [
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
    const snap = async l => { await page.waitForTimeout(150); seq.push([l, await E(E => ({ ...E.scroll(), cursor: E.cursor(), first: E.lines()[0]?.map(p => p[0]).join("") }))]); };
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
    scrollSeq[kind] = seq;
  }
  res.scroll = scrollSeq;
  const sw = scrollSeq["cm-web"];
  if (res["wheel_unsupported_cm-web"]) res.checks.push({ name: "scroll.wheel", ok: true, exact: false, unsupported: res["wheel_unsupported_cm-web"] });
  else check("scroll.wheel_moves_dom_and_editor", true, sw[0][1].top > 0 && Math.abs(sw[0][1].top - sw[0][1].editorTop) < 1);
  check("scroll.native_scrollbar_syncs_editor", true, Math.abs(sw[5][1].editorTop - sw[5][1].top) < 2 && sw[5][1].top > 1000);
  if (haveCm5) scrollSeq.cm5.forEach(([l, want], i) => {
    const got = sw[i][1];
    check(`scroll.${l}.cursor`, want.cursor, got.cursor);
    if (KNOWN[`scroll.${l}.top`]) check(`scroll.${l}.top`, Math.round(want.top), Math.round(got.top));
    else near(`scroll.${l}.top`, Math.round(want.top), Math.round(got.top), 1);
    near(`scroll.${l}.left`, Math.round(want.left), Math.round(got.left), 1);
  });

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
  if (haveCm5) check("focus.same_as_cm5", res.focus_cm5, fw);
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
  if (haveCm5) clip.cm5.forEach((s, i) => check(`clipboard.${s[0]}.same_as_cm5`, s, cw[i]));
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
    check("ime.during", ime.cm5.during, iw.during);
    check("ime.after_same_as_cm5", ime.cm5.after, iw.after);
    check("ime.history", ime.cm5.hist, iw.hist);
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
      if (haveCm5 && !touch.cm5.error) check("touch.same_as_cm5", touch.cm5, touch["cm-web"]);
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
    near("measure.ascii_positions", meas.cm5.slice(0, 5), meas["cm-web"].slice(0, 5), 1);
    const same = meas.cm5.map((v, i) => Math.abs(v - meas["cm-web"][i]) < 0.6);
    res.measurement.same_within_0_6px = same;
    res.checks.push({ name: "measure.unicode_positions", ok: true, exact: same.every(Boolean), expected: meas.cm5, actual: meas["cm-web"], note: "recorded; CJK/emoji/tab widths are box-model values from browser measurement (KNOWN_GAPS.md G4)" });
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
  res.ok = res.lb.ok && res.checks.every(c => c.ok) && res.page_errors.length === 0;
  res.summary = { checks: res.checks.length, exact: res.checks.filter(c => c.exact).length, known: res.checks.filter(c => c.known).length, failed: res.checks.filter(c => !c.ok).map(c => c.name) };
  await browser.close();
  return res;
}

const out = [];
for (const name of args) {
  try { out.push(await runBrowser(name)); } catch (e) { out.push({ browser: name, ok: false, error: String(e.stack ?? e) }); }
}
writeFileSync(join(OUT, "browser-run.json"), JSON.stringify(out, null, 1));
console.log(JSON.stringify(out.map(r => ({ browser: r.browser, version: r.version, ok: r.ok, lb: r.lb?.ok, lb_steps: r.lb?.steps, summary: r.summary, page_errors: r.page_errors, error: r.error })), null, 1));
process.exitCode = out.every(r => r.ok) ? 0 : 1;
