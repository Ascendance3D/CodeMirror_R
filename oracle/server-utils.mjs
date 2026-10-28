import { join, normalize, relative, isAbsolute, sep } from "node:path";
import { realpathSync } from "node:fs";

export function safeResolve(urlPath, rootDir) {
   let decoded;
   try {
      decoded = decodeURIComponent(urlPath);
   } catch {
      return { status: 400, path: null };
   }

   const path = normalize(join(rootDir, decoded));

   // A normal check to handle 403 on non-existent targets safely
   const preRel = relative(rootDir, path);
   if (preRel === ".." || preRel.startsWith(".." + sep) || isAbsolute(preRel)) {
      return { status: 403, path: null };
   }

   let realPath, realRoot;
   try {
      realRoot = realpathSync(rootDir);
      realPath = realpathSync(path);
   } catch {
      // If the file does not exist, return 404
      return { status: 404, path: null };
   }

   const rel = relative(realRoot, realPath);
   if (rel === ".." || rel.startsWith(".." + sep) || isAbsolute(rel)) {
      return { status: 403, path: null };
   }

   return { status: 200, path: realPath };
}
