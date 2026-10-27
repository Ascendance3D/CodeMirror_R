// The L-B wasm runtime probe (20 steps in 5 groups), run through cm-web's public
// API. It runs in Node (development host) and in browser pages. `load()` returns
// a fresh module instance (its own clock state), like the probe's per-group
// `WebAssembly.instantiate`. Each scenario returns 0, or the number of the failed
// check with its message in `fail.msg`. The check numbers are the probe's.

function scenarios(api) {
  const fail = { msg: "" };
  const check = (n, ok, msg) => {
    if (!ok) {
      fail.msg = `check ${n}: ${typeof msg === "function" ? msg() : msg}`;
      throw n;
    }
  };
  const run = f => {
    try { return f(); } catch (n) { if (typeof n === "number") return n; throw n; }
  };
  const ed = (text, mode) => new api.WebEditor(text, mode, { metrics: "grid" });
  const eq = (a, b) => JSON.stringify(a) === JSON.stringify(b);

  // Construct, insert, delete, move, indent, undo/redo, Unicode, destroy.
  function editing() {
    const cm = ed("int f() {\n\n}", "text/x-csrc");
    check(1, cm.getValue() === "int f() {\n\n}", () => `construct: ${cm.getValue()}`);
    cm.setCursor(1, 0);
    for (const c of ["a", "b", "c"]) cm.typeText(c);
    const depth = cm.historySize()[0];
    check(2, cm.getValue() === "int f() {\nabc\n}", () => `insert: ${cm.getValue()}`);
    check(3, cm.execCommand("delCharBefore"), "delCharBefore");
    check(4, cm.getValue() === "int f() {\nab\n}", () => `delete: ${cm.getValue()}`);
    check(5, cm.execCommand("goLineStart"), "goLineStart");
    check(6, eq(Array.from(cm.getCursor()), [1, 0]), () => `move: ${cm.getCursor()}`);
    cm.setSelection(1, 0, 1, 2);
    check(7, cm.getSelection() === "ab", () => `select: ${cm.getSelection()}`);
    cm.indentLine(1, "smart");
    check(8, cm.getValue() === "int f() {\n  ab\n}", () => `indent: ${cm.getValue()}`);
    check(9, cm.execCommand("undo"), "undo");
    check(10, cm.getValue() === "int f() {\nab\n}", () => `undo indent: ${cm.getValue()}`);
    check(11, cm.execCommand("redo"), "redo");
    check(12, cm.getValue() === "int f() {\n  ab\n}", () => `redo: ${cm.getValue()}`);
    while (cm.historySize()[0] > 0) cm.execCommand("undo");
    check(13, cm.getValue() === "int f() {\n\n}", () => `undo all: ${cm.getValue()}`);
    // Unicode: astral and combining text, cursor in UTF-16 units like CM5.
    const u = ed("ü😀\n日本", "text/plain");
    u.setCursor(0, 3);
    u.typeText("é");
    check(14, u.getValue() === "ü😀é\n日本", () => `unicode insert: ${u.getValue()}`);
    check(15, u.execCommand("delCharBefore") && u.execCommand("delCharBefore"), "unicode delete");
    check(16, u.getValue() === "ü\n日本", () => `astral deleted as one char: ${u.getValue()}`);
    u.destroy();
    cm.destroy();
    return depth;
  }

  return {
    fail,
    scenario_no_clock: () => run(() => {
      check(100, api.clockStatus() === 2, () => `status ${api.clockStatus()}`);
      const before = api.missingReads();
      const cm = ed("int x;\n", "text/x-csrc");
      check(101, api.missingReads() > before, "construction read no clock");
      cm.destroy();
      const depth = editing();
      check(102, depth === 3, `unknown time: one event per keystroke, got ${depth}`);
      return 0;
    }),
    scenario_host_clock: expect => run(() => {
      check(200, api.clockStatus() === 0, () => `status ${api.clockStatus()}`);
      const depth = editing();
      check(201, depth === expect, `undo depth ${depth}, expected ${expect}`);
      return 0;
    }),
    scenario_modes: () => run(() => {
      const reg = new api.ModeRegistry();
      check(300, !reg.hasMode("toml"), "fresh registry has toml");
      reg.register("toml");
      check(301, reg.hasMode("toml") && reg.hasMime("text/x-toml"), "toml registered");
      const styles = Array.from(reg.runMode('[a]\nk = "ü😀" # c', "text/x-toml")).map(([t, s]) => `${t}|${s ?? ""}`);
      // Same tokens as the native test crates/cm-view/tests/portability.rs.
      const want = ["[a]|atom", "\n|", "k|property", " |", "=|", " |", '"ü😀"|string', " |", "# c|comment"];
      check(302, eq(styles, want), () => `runmode tokens ${JSON.stringify(styles)}`);
      const cm = api.WebEditor.withRegistry("x = 1", "text/plain", reg, { metrics: "grid" });
      check(303, cm.modeName() === "null", () => `initial mode ${cm.modeName()}`);
      cm.setMode("text/x-toml");
      check(304, cm.modeName() === "toml", () => `selected mode ${cm.modeName()}`);
      const lt = Array.from(cm.lineTokens(0));
      check(305, eq(lt[0], ["x", "property"]), () => `line tokens ${JSON.stringify(lt)}`);
      const t2 = Array.from(ed("int x = 1; // c", "text/x-csrc").lineTokens(0)).map(t => t[1]);
      check(306, t2[0] === "type" && t2[t2.length - 1] === "comment", () => `clike tokens ${JSON.stringify(t2)}`);
      cm.destroy();
      return 0;
    }),
    scenario_repeat: n => run(() => {
      const live = api.liveEditors();
      for (let i = 0; i < n; i++) {
        const cm = ed("fn main() {}\n", "rust");
        cm.typeText("x");
        check(400, cm.getValue() === "xfn main() {}\n", () => `iteration ${i}: ${cm.getValue()}`);
        check(401, cm.execCommand("undo") && cm.getValue() === "fn main() {}\n", `undo at ${i}`);
        cm.destroy();
      }
      check(402, api.liveEditors() === live, () => `live editors ${api.liveEditors()}, expected ${live}`);
      return 0;
    }),
    // The probe's 504 checked that std::fs returns an error on wasm. The adapter has
    // no std::fs path; its file host is the page's functions, so 504 checks that a
    // failing host read is an error, not a trap.
    scenario_files: () => run(() => {
      const files = new Map([["/doc.wrl", new TextEncoder().encode("#VRML V2.0 utf8\r\nShape {}\r\n")]]);
      const host = { read: p => files.get(p) ?? null, write: (p, b) => { files.set(p, b.slice()); } };
      let app;
      try { app = api.WebEditor.open("/doc.wrl", host, { metrics: "grid" }); } catch (e) { check(500, false, `open failed: ${e}`); }
      check(501, app.modeName() === "vrml", () => `mode ${app.modeName()}`);
      app.setCursor(1, 0);
      app.typeText("X");
      let saved = true;
      try { app.save(); } catch { saved = false; }
      check(502, saved, "save");
      const bytes = new TextDecoder().decode(files.get("/doc.wrl"));
      check(503, bytes === "#VRML V2.0 utf8\r\nXShape {}\r\n", () => `bytes ${JSON.stringify(bytes)}`);
      let err = null;
      try { api.WebEditor.open("/x.wrl", { read: () => { throw new Error("EACCES"); } }, {}); } catch (e) { err = String(e.message ?? e); }
      check(504, err !== null && err.includes("EACCES"), `failing host read: ${err}`);
      app.destroy();
      return 0;
    }),
  };
}

export async function runLB(load, label = "?") {
  const results = { label, groups: [] };
  async function group(name, body) {
    const api = await load();
    const s = scenarios(api);
    const steps = [];
    const step = (what, f, expect) => {
      let got, ok;
      try { got = f(); ok = expect(got); } catch (err) { got = `TRAP ${err?.name}: ${err?.message ?? err}`; ok = false; }
      const detail = typeof got === "number" && got !== 0 && !ok && s.fail.msg ? s.fail.msg : undefined;
      steps.push({ what, got: typeof got === "number" && Number.isNaN(got) ? "NaN" : got, ok, ...(detail ? { detail } : {}) });
    };
    await body(api, s, step);
    results.groups.push({ name, ok: steps.every(x => x.ok), steps });
  }
  const zero = r => r === 0;
  await group("no host clock (missing initialization)", (api, s, step) => {
    step("clock_status == Unavailable(2)", () => api.clockStatus(), r => r === 2);
    step("clock_now is NaN", () => api.clockNow(), r => Number.isNaN(r));
    step("scenario_no_clock (construct/insert/delete/move/indent/undo/redo/unicode/destroy, 1 event per key)", () => s.scenario_no_clock(), zero);
    step("missing_reads > 0", () => api.missingReads(), r => r > 0);
  });
  await group("deterministic host clock (+100 ms per read)", (api, s, step) => {
    let v = 1e12;
    api.installClock(() => (v += 100));
    step("clock_status == Host(0)", () => api.clockStatus(), r => r === 0);
    step("time values pass through and advance", () => { const a = api.clockNow(), b = api.clockNow(); return b - a; }, r => r === 100);
    step("scenario_host_clock(1): typing merges in the window", () => s.scenario_host_clock(1), zero);
    step("missing_reads == 0", () => api.missingReads(), r => r === 0);
  });
  await group("deterministic host clock (+1300 ms per read)", (api, s, step) => {
    let v = 1e12;
    api.installClock(() => (v += 1300));
    step("scenario_host_clock(3): typing outside the 1250 ms window splits", () => s.scenario_host_clock(3), zero);
  });
  await group("host Date.now", (api, s, step) => {
    api.installClock();
    step("clock_now close to Date.now", () => Math.abs(api.clockNow() - Date.now()), r => r < 1000);
    step("scenario_host_clock(1)", () => s.scenario_host_clock(1), zero);
    step("scenario_modes (register, select, tokenize, Unicode tokens)", () => s.scenario_modes(), zero);
    step("scenario_repeat(300) construct/destroy", () => s.scenario_repeat(300), zero);
    step("scenario_files (memory file host, failing host read errors without trap)", () => s.scenario_files(), zero);
  });
  await group("host callback lifetime", (api, s, step) => {
    let t = () => 5e11;
    api.installClock(() => t());
    step("host time 5e11", () => api.clockNow(), r => r === 5e11);
    t = () => NaN;
    step("host returns NaN -> unknown (NaN), counted", () => [api.clockNow(), api.missingReads()], r => Number.isNaN(r[0]) && r[1] === 1);
    step("scenario_host_clock(3) with host time unknown", () => s.scenario_host_clock(3), zero);
    api.removeClock();
    step("removed -> Unavailable(2)", () => api.clockStatus(), r => r === 2);
    let called = 0;
    t = () => { called++; return 1; };
    step("no host call after removal", () => (api.clockNow(), called), r => r === 0);
    api.installClock(() => t());
    step("reinstalled -> Host(0), value 1", () => [api.clockStatus(), api.clockNow()], r => r[0] === 0 && r[1] === 1);
  });
  results.steps = results.groups.reduce((n, g) => n + g.steps.length, 0);

  // Adapter clock cases beyond the probe (L-C section 7).
  const extra = [];
  {
    const api = await load();
    const x = (what, f, expect) => {
      let got, ok;
      try { got = f(); ok = expect(got); } catch (err) { got = `TRAP ${err?.name}: ${err?.message ?? err}`; ok = false; }
      extra.push({ what, got: typeof got === "number" && Number.isNaN(got) ? "NaN" : got, ok });
    };
    api.installClock(() => { throw new Error("clock gone"); });
    x("throwing clock -> unknown, counted, no trap", () => [api.clockNow(), api.missingReads()], r => Number.isNaN(r[0]) && r[1] === 1);
    api.installClock(() => "soon");
    x("non-number clock -> unknown", () => api.clockNow(), r => Number.isNaN(r));
    api.installClock(() => Infinity);
    x("infinite clock -> unknown", () => api.clockNow(), r => Number.isNaN(r));
    x("editing with invalid time: 1 event per key", () => {
      const cm = new api.WebEditor("", "text/plain", {});
      for (const c of "abc") cm.typeText(c);
      const d = cm.historySize()[0];
      cm.destroy();
      return d;
    }, r => r === 3);
    let v = 2e12, old = 0;
    api.installClock(() => (old++, v));
    api.installClock(() => (v += 10));
    x("replacement: old clock no longer called", () => (api.clockNow(), old), r => r === 0);
    x("history merges with the replacement (+10 ms)", () => {
      const cm = new api.WebEditor("", "text/plain", {});
      for (const c of "abc") cm.typeText(c);
      cm.execCommand("undo");
      const r = [cm.historySize()[0], cm.getValue()];
      cm.destroy();
      return r;
    }, r => r[0] === 0 && r[1] === "");
    x("undo/redo after merge", () => {
      const cm = new api.WebEditor("", "text/plain", {});
      for (const c of "ab") cm.typeText(c);
      cm.execCommand("undo");
      cm.execCommand("redo");
      const r = cm.getValue();
      cm.destroy();
      return r;
    }, r => r === "ab");
    x("destroying editors keeps the clock", () => [api.liveEditors(), api.clockStatus()], r => r[0] === 0 && r[1] === 0);
    x("calls after destroy throw, not trap", () => {
      const cm = new api.WebEditor("a", "text/plain", {});
      cm.destroy();
      try { cm.getValue(); return "no throw"; } catch (e) { return String(e.message); }
    }, r => r.includes("destroyed"));
    x("re-entrant call from the clock throws busy, no trap", () => {
      const cm = new api.WebEditor("", "text/plain", {});
      let msg = "";
      api.installClock(() => { try { cm.getValue(); } catch (e) { msg = String(e.message); } return 1; });
      cm.typeText("a");
      api.installClock();
      const r = [msg, cm.getValue()];
      cm.destroy();
      return r;
    }, r => r[0].includes("busy") && r[1] === "a");
  }
  results.adapter_clock = extra;
  results.ok = results.groups.every(g => g.ok) && extra.every(e => e.ok);
  return results;
}
