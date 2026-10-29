// The browser test page: runs the L-B scenarios through cm-web in this page, then
// exposes `window.drivers` so browser-run.mjs can drive cm-web and (when the CM5
// build is served under /cm5/) the pinned CodeMirror 5 with the same real input.
import { runLB } from "./lb_scenarios.mjs";

const module = await WebAssembly.compileStreaming(fetch("./pkg/cm_web_bg.wasm"));
let n = 0;
const load = async () => {
  const api = await import(`./pkg/cm_web.js?i=${n++}`);
  await api.default({ module_or_path: module });
  return api;
};
window.__lb = await runLB(load, navigator.userAgent);
const api = await load();
window.api = api;

const host = document.getElementById("host");
const arr = a => Array.from(a, x => (Array.isArray(x) || x?.length !== undefined && typeof x !== "string" ? Array.from(x) : x));

// The rendered token spans of each rendered line, from the DOM (both editors render
// `pre.CodeMirror-line > span > (text | span.cm-*)`).
function domLines(root) {
  return Array.from(root.querySelectorAll(".CodeMirror-code pre.CodeMirror-line")).map(pre => {
    const out = [];
    const walk = node => {
      for (const c of node.childNodes) {
        if (c.nodeType === 3) { if (c.data.replace(/​/g, "")) out.push([c.data, ""]); continue; }
        if (c.nodeType !== 1) continue;
        const cls = (c.className || "").split(/\s+/).filter(x => x.startsWith("cm-")).sort().join(" ");
        if (c.children.length === 0) { const t = c.textContent.replace(/​/g, ""); if (t) out.push([t, cls]); }
        else walk(c);
      }
    };
    walk(pre);
    // Merge adjacent pieces with the same classes (CM5 and cm-web may split text nodes differently).
    const merged = [];
    for (const p of out) { const l = merged[merged.length - 1]; if (l && l[1] === p[1]) l[0] += p[0]; else merged.push([...p]); }
    return merged;
  });
}

function rustDriver(text, mode, opts) {
  host.textContent = "";
  const ed = new api.WebEditor(text, mode, opts);
  ed.attach(host);
  ed.renderNow();
  const root = host.querySelector(".CodeMirror");
  const scroller = root.querySelector(".cm-web-scroll");
  return {
    kind: "cm-web", root, ed,
    scroller: () => scroller,
    field: () => root.querySelector("textarea"),
    value: () => ed.getValue(),
    cursor: () => arr(ed.getCursor()),
    sels: () => arr(ed.listSelections()),
    hist: () => arr(ed.historySize()),
    focused: () => ed.hasFocus(),
    scroll: () => ({ top: scroller.scrollTop, left: scroller.scrollLeft, editorTop: ed.scrollInfo().top, editorLeft: ed.scrollInfo().left }),
    render: () => ed.renderNow(),
    // Window coordinates of the middle of the cursor box at (line, ch).
    point: (line, ch) => {
      const c = ed.cursorCoords(line, ch), r = scroller.getBoundingClientRect(), m = ed.metrics(), s = ed.scrollInfo();
      return { x: r.left + m.gutterWidth + c.left - s.left, y: r.top + c.top - s.top + (c.bottom - c.top) / 2 };
    },
    local: (line, ch) => { const c = ed.cursorCoords(line, ch); return { left: c.left, top: c.top, bottom: c.bottom }; },
    lines: () => domLines(root),
    destroy: () => ed.destroy(),
  };
}

function cm5Driver(text, mode, opts) {
  host.textContent = "";
  const cm = window.CodeMirror(host, { value: text, mode, ...opts });
  const root = cm.getWrapperElement();
  return {
    kind: "cm5", root, cm,
    scroller: () => cm.getScrollerElement(),
    field: () => cm.getInputField(),
    value: () => cm.getValue(),
    cursor: () => { const c = cm.getCursor(); return [c.line, c.ch]; },
    sels: () => cm.listSelections().map(r => [r.anchor.line, r.anchor.ch, r.head.line, r.head.ch]),
    hist: () => { const h = cm.historySize(); return [h.undo, h.redo]; },
    focused: () => cm.hasFocus(),
    scroll: () => { const s = cm.getScrollInfo(); return { top: s.top, left: s.left, editorTop: s.top, editorLeft: s.left }; },
    render: () => cm.refresh(),
    point: (line, ch) => { const c = cm.cursorCoords({ line, ch }, "window"); return { x: c.left, y: (c.top + c.bottom) / 2 }; },
    local: (line, ch) => { const c = cm.cursorCoords({ line, ch }, "local"); return { left: c.left, top: c.top, bottom: c.bottom }; },
    lines: () => domLines(root),
    destroy: () => { root.remove(); },
  };
}

window.drivers = {
  make(kind, text, mode, opts = {}) {
    if (window.E) { try { window.E.destroy(); } catch {} }
    window.E = kind === "cm5" ? cm5Driver(text, mode, opts) : rustDriver(text, mode, opts);
    return true;
  },
};
window.__ready = true;
document.title = window.__lb.ok ? "PASS" : "FAIL";
