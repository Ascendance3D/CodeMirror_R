// node node-run.mjs <pkg dir from wasm-bindgen --target web>
// Development host only: runs the L-B scenarios through cm-web's API in Node.
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { runLB } from "./lb_scenarios.mjs";

const pkg = resolve(process.argv[2]);
const module = await WebAssembly.compile(readFileSync(join(pkg, "cm_web_bg.wasm")));
let n = 0;
// A fresh glue module and instance per group: `?i=` makes a new ES module record.
const load = async () => {
  const api = await import(`${pathToFileURL(join(pkg, "cm_web.js")).href}?i=${n++}`);
  await api.default({ module_or_path: module });
  return api;
};
const imports = WebAssembly.Module.imports(module).map(i => `${i.module}.${i.name}`);
const r = await runLB(load, `node ${process.version}`);
r.import_count = imports.length;
console.log(JSON.stringify(r, null, 1));
process.exitCode = r.ok ? 0 : 1;
