/**
 * C0 oracle: run the upstream CodeMirror 5 browser test suite headless.
 *
 * Replaces upstream test/run.js, which needs puppeteer 1.x and its own old chromium.
 * Uses the playwright that x_ite-tests already installed.
 *
 *   node oracle/cm5-run.mjs [filter]      # filter = test name prefix, as in test/index.html#
 *   node oracle/cm5-run.mjs --json out.json
 */
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = normalize (join (fileURLToPath (import.meta.url), ".."));
const ROOT = join (HERE, "cm5-build");
const { chromium } = createRequire (join (HERE, "../../../x_ite-tests/")) ("playwright");

const TYPES = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css" };

const args = process .argv .slice (2);
const jsonAt = args .indexOf ("--json");
const jsonOut = jsonAt >= 0 ? args .splice (jsonAt, 2) [1] : null;
const filter = args [0] || "";

const server = createServer (async (req, res) =>
{
   const path = normalize (join (ROOT, decodeURIComponent (new URL (req .url, "http://x") .pathname)));

   if (!path .startsWith (ROOT)) { res .writeHead (403); return res .end (); }

   try
   {
      const body = await readFile (path);
      res .writeHead (200, { "Content-Type": TYPES [extname (path)] || "application/octet-stream", "Cache-Control": "no-store" });
      res .end (body);
   }
   catch { res .writeHead (404); res .end (); }
});

await new Promise (r => server .listen (0, "127.0.0.1", r));

const browser = await chromium .launch ({ headless: true });
const page    = await browser .newPage ({ viewport: { width: 1200, height: 900 } });

page .on ("dialog", d => d .dismiss ());
await page .addInitScript (() => window .automatedTests = true);
await page .goto (`http://127.0.0.1:${server .address () .port}/test/index.html#${filter}`);
await page .waitForFunction (() => window .done, null, { timeout: 600000, polling: 200 });

const result = await page .evaluate (() => ({
   failed:  window .failed,
   errors:  window .errored,
   output:  document .getElementById ("output") .innerText,
   status:  document .getElementById ("status") .innerText,
   ran:     Number (document .getElementById ("progress_ran") .textContent),
   total:   Number (document .getElementById ("progress_total") .textContent .replace (/\D+/g, "")),
}));

await browser .close ();
server .close ();

for (const e of result .errors) console .log (e);
console .log (result .output);
console .log (`${result .status} (ran ${result .ran} of ${result .total})`);

if (jsonOut)
   await writeFile (jsonOut, JSON .stringify (result, null, 1));

process .exit (result .failed > 0 || result .errors .length || result .ran === 0 ? 1 : 0);
