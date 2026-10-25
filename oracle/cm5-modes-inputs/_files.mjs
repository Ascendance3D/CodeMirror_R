// Shared helpers for the C4 input lists: recursive file listing (sorted, by path).
import { readdirSync } from "node:fs";
import { join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = normalize (join (fileURLToPath (import.meta.url), "..", "..", ".."));

export function listFiles (dir, exts, skip = [])
{
   const out = [];
   (function walk (d)
   {
      for (const e of readdirSync (d, { withFileTypes: true }) .sort ((a, b) => a .name < b .name ? -1 : a .name > b .name ? 1 : 0))
      {
         const p = join (d, e .name);
         if (e .isDirectory ()) { if (!skip .includes (e .name)) walk (p); }
         else if (exts .some (x => e .name .endsWith (x))) out .push (p);
      }
   }) (join (ROOT, dir));
   return out;
}
