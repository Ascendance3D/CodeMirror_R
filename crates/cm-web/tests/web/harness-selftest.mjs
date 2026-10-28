// CM5_PLAYWRIGHT_FROM=<dir with node_modules/playwright>/ \
//   node harness-selftest.mjs --pkg <wasm-bindgen pkg dir> --cm5 <built cm5 dir> --out <dir> [browser]
//
// Checks browser-run.mjs itself: each case runs it once (default chromium) with a good
// reference, a broken reference or an injected fault, and asserts the exit status, the
// browser's status and reason, and that every process the run started is gone after it
// exits. On Linux the run's process tree is sampled from /proc every 25 ms while it runs, and each
// process seen (pid and start time) must have exited when the run ends; elsewhere the
// process check is reported as not available. Development-only Node use.
import { spawn } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const opt = k => { const i = args.indexOf(k); if (i < 0) return null; const v = args[i + 1]; args.splice(i, 2); return v; };
const PKG = resolve(opt("--pkg") ?? "");
const CM5 = resolve(opt("--cm5") ?? "");
const OUT = resolve(opt("--out") ?? "harness-selftest");
const BROWSER = args[0] ?? "chromium";
if (!process.env.CM5_PLAYWRIGHT_FROM) { console.log("UNAVAILABLE: CM5_PLAYWRIGHT_FROM is not set (Playwright)"); process.exit(2); }
if (!existsSync(join(CM5, "lib/codemirror.js"))) { console.log(`BLOCKED: --cm5 ${CM5} has no lib/codemirror.js`); process.exit(2); }
mkdirSync(OUT, { recursive: true });

// Reference fixtures, made from the good build.
const fx = join(OUT, "fixtures");
rmSync(fx, { recursive: true, force: true });
const fixture = (name, edit) => { const d = join(fx, name); mkdirSync(d, { recursive: true }); for (const f of ["lib", "mode"]) cpSync(join(CM5, f), join(d, f), { recursive: true }); edit?.(d); return d; };
const js = d => join(d, "lib/codemirror.js");
mkdirSync(join(fx, "empty"), { recursive: true });
const unrelated = join(fx, "unrelated"); mkdirSync(unrelated, { recursive: true }); writeFileSync(join(unrelated, "README"), "not a CM5 build\n");
const noEntry = fixture("no-entry", d => rmSync(js(d)));
const wrongVersion = fixture("wrong-version", d => writeFileSync(js(d), readFileSync(js(d), "utf8").replace('CodeMirror.version = "5.65.21"', 'CodeMirror.version = "5.65.20"')));
const throws = fixture("throws-on-load", d => writeFileSync(js(d), `throw new Error("injected load failure");\n${readFileSync(js(d), "utf8")}`));
// Defines what the mode files need, and the version, but not the editor API.
const partialApi = fixture("partial-api", d => writeFileSync(js(d), `// CodeMirror.version = "5.65.21"
window.CodeMirror = function () {};
CodeMirror.version = "5.65.21";
CodeMirror.modes = {}; CodeMirror.mimeModes = {};
CodeMirror.defineMode = (n, f) => { CodeMirror.modes[n] = f; };
CodeMirror.defineMIME = (m, s) => { CodeMirror.mimeModes[m] = s; };
CodeMirror.registerHelper = () => {}; CodeMirror.Pass = {}; CodeMirror.StringStream = function () {};
CodeMirror.prototype.getValue = function () { return ""; };
`));
const modeDiffers = fixture("mode-differs", d => writeFileSync(join(d, "mode/clike/clike.js"), readFileSync(join(d, "mode/clike/clike.js"), "utf8") + "\n// edited\n"));

// [name, browser-run arguments, expected exit, expected status, reason pattern, expect a launched browser]
const ref = d => ["--cm5", d];
const CASES = [
  ["pass", ref(CM5), 0, "PASS", null, true],
  ["no-reference", [], 2, "BLOCKED", /not supplied/, false],
  ["missing-dir", ref(join(fx, "does-not-exist")), 2, "BLOCKED", /does not exist/, false],
  ["empty-dir", ref(join(fx, "empty")), 2, "BLOCKED", /is empty/, false],
  ["unrelated-dir", ref(unrelated), 2, "BLOCKED", /lacks lib\/codemirror.css/, false],
  ["missing-entry", ref(noEntry), 2, "BLOCKED", /lacks lib\/codemirror.js/, false],
  ["mode-differs", ref(modeDiffers), 2, "BLOCKED", /mode\/clike\/clike.js differ/, false],
  ["wrong-version", ref(wrongVersion), 2, "BLOCKED", /does not declare CodeMirror.version/, false],
  ["fails-to-load", ref(throws), 2, "BLOCKED", /window.CodeMirror is undefined/, true],
  ["partial-api", ref(partialApi), 2, "BLOCKED", /CM5 API lacks getCursor/, true],
  // No reference: the cm-web-only checks still run and are still required (BLOCKED, not PASS: nothing was compared).
  ["no-oracle", ["--no-cm5"], 2, "BLOCKED", /--no-cm5/, true, r => r.integrity?.complete === true && r.integrity.web_only?.window_scroll?.ran === 2 && !r.integrity.sections && r.summary.differential === 0],
  ["no-oracle-drops-web-check", ["--no-cm5", "--inject", "drop-web-check"], 1, "FAIL", null, true, r => r.integrity?.incomplete?.join() === "window_scroll (cm-web only)"],
  ["oracle-drops-web-check", [...ref(CM5), "--inject", "drop-web-check"], 1, "FAIL", null, true, r => r.integrity?.incomplete?.join() === "window_scroll (cm-web only)" && r.integrity.sections.window_scroll.complete],
  ["compare-fail", [...ref(CM5), "--inject", "compare-fail"], 1, "FAIL", null, true, r => r.summary.failed.includes("keys.type")],
  ["partial-registration", [...ref(CM5), "--inject", "drop-section"], 1, "FAIL", null, true, r => r.integrity?.incomplete?.join() === "mouse"],
  ["navigation-fails", [...ref(CM5), "--inject", "nav-fail"], 1, "FAIL", /page\.goto: .*(ERR_NAME_NOT_RESOLVED|NS_ERROR_UNKNOWN_HOST|Could not resolve|Error resolving)/, true],
  ["page-exception", [...ref(CM5), "--inject", "page-throw"], 1, "FAIL", /injected page exception/, true],
  ["rejected-promise", [...ref(CM5), "--inject", "reject"], 1, "FAIL", /injected rejected promise/, true],
  ["context-fails", [...ref(CM5), "--inject", "context-fail"], 1, "FAIL", /injected-missing-storage-state/, true],
  ["launch-fails", [...ref(CM5), "--inject", "launch-fail"], 2, "BLOCKED", /did not launch/, false],
  ["timeout", [...ref(CM5), "--inject", "hang", "--browser-timeout", "20"], 1, "FAIL", /timed out after 20 s/, true],
  ["unknown-browser", ref(CM5), 2, "UNSUPPORTED", /no browser profile/, false, null, "netscape"],
  ["no-browser", ref(CM5), 2, "BLOCKED", /no browser named/, false, null, "none"],
];

// Linux: the descendants of `root` with their start times, from /proc.
const linux = process.platform === "linux";
function tree(root) {
  const kids = new Map();
  for (const p of readdirSync("/proc").filter(d => /^\d+$/.test(d))) {
    try {
      const st = readFileSync(`/proc/${p}/stat`, "utf8"), f = st.slice(st.lastIndexOf(")") + 2).split(" ");
      const ppid = +f[1];
      if (!kids.has(ppid)) kids.set(ppid, []);
      kids.get(ppid).push({ pid: +p, start: f[19], comm: st.slice(st.indexOf("(") + 1, st.lastIndexOf(")")) });
    } catch {}
  }
  const out = [], walk = pid => { for (const k of kids.get(pid) ?? []) { out.push(k); walk(k.pid); } };
  walk(root);
  return out;
}
const alive = p => { try { const st = readFileSync(`/proc/${p.pid}/stat`, "utf8"); return st.slice(st.lastIndexOf(")") + 2).split(" ")[19] === p.start; } catch { return false; } };

function run([name, extra, , , , , , browser]) {
  const dir = join(OUT, name);
  mkdirSync(dir, { recursive: true });
  const argv = [join(HERE, "browser-run.mjs"), "--pkg", PKG, "--out", dir, ...extra, ...(browser === "none" ? [] : [browser ?? BROWSER])];
  return new Promise(done => {
    const t0 = Date.now();
    const child = spawn(process.execPath, argv, { stdio: ["ignore", "pipe", "pipe"] });
    const seen = new Map();
    let stdout = "", stderr = "";
    child.stdout.on("data", d => { stdout += d; });
    child.stderr.on("data", d => { stderr += d; });
    const sample = () => { if (linux) for (const p of tree(child.pid)) seen.set(`${p.pid}:${p.start}`, p); };
    const poll = setInterval(sample, 25);
    // A run that does not exit by itself within 3 minutes is killed and fails the case.
    const cap = setTimeout(() => { child.kill("SIGKILL"); }, 180000);
    child.on("exit", (code, signal) => {
      clearInterval(poll); clearTimeout(cap);
      const exited = Date.now();
      // Give the browser's own exit a moment, then look for anything the run left behind.
      setTimeout(() => {
        const left = [...seen.values()].filter(alive);
        writeFileSync(join(dir, "stdout.json"), stdout);
        writeFileSync(join(dir, "stderr.log"), stderr);
        done({ name, code, signal, seconds: (exited - t0) / 1000, processes: linux ? { seen: seen.size, browsers: [...new Set([...seen.values()].map(p => p.comm))], left: left.map(p => `${p.pid} ${p.comm}`) } : "not available off Linux" });
      }, 2000);
    });
  });
}

async function pool(items, n, f) {
  const out = new Array(items.length);
  let next = 0;
  await Promise.all(Array.from({ length: n }, async () => { while (next < items.length) { const i = next++; out[i] = await f(items[i]); } }));
  return out;
}

const ran = await pool(CASES, 6, run);
const report = CASES.map((c, i) => {
  const [name, , wantCode, wantStatus, reason, launched, extra] = c;
  const r = ran[i];
  let result = null;
  try { result = JSON.parse(readFileSync(join(OUT, name, "browser-run.json"), "utf8"))[0]; } catch {}
  const why = result?.blocked ?? result?.error ?? "";
  const problems = [];
  if (r.signal) problems.push(`killed by ${r.signal} (did not exit by itself)`);
  if (r.code !== wantCode) problems.push(`exit ${r.code}, want ${wantCode}`);
  if (result?.status !== wantStatus) problems.push(`status ${result?.status}, want ${wantStatus}`);
  if (wantStatus !== "PASS" && result?.ok !== false) problems.push(`ok ${result?.ok}, want false`);
  if (reason && !reason.test(why)) problems.push(`reason ${JSON.stringify(why.split("\n")[0])} does not match ${reason}`);
  if (extra && !(result && extra(result))) problems.push("case-specific assertion failed");
  if (wantStatus === "PASS" && !(result?.integrity?.complete && result.summary.differential === 88)) problems.push("integrity not complete or differential != 88");
  if (launched && result?.cleanup?.browser_closed !== true) problems.push(`cleanup ${JSON.stringify(result?.cleanup)}`);
  if (linux && launched && r.processes.seen === 0) problems.push("no browser process was observed");
  if (linux && r.processes.left.length) problems.push(`left running: ${r.processes.left.join(", ")}`);
  return { case: name, ok: problems.length === 0, exit: r.code, status: result?.status, reason: why.split("\n")[0], seconds: r.seconds, cleanup: result?.cleanup, integrity: result?.integrity?.incomplete ?? result?.integrity?.complete, checks: result?.summary?.checks, processes: r.processes, problems };
});
writeFileSync(join(OUT, "harness-selftest.json"), JSON.stringify(report, null, 1));
for (const r of report) console.log(`${r.ok ? "ok  " : "FAIL"} ${r.case.padEnd(22)} exit=${r.exit} ${String(r.status).padEnd(11)} ${r.seconds.toFixed(1).padStart(6)}s procs=${linux ? `${r.processes.seen} seen/${r.processes.left.length} left` : "n/a"} ${r.problems.join("; ")}`);
const failed = report.filter(r => !r.ok).length;
console.log(`${report.length - failed}/${report.length} harness cases passed (${BROWSER})`);
process.exitCode = failed ? 1 : 0;
