import { join, normalize, relative, isAbsolute } from "node:path";

export function safeResolve(urlPath, rootDir) {
   let decoded;
   try {
      decoded = decodeURIComponent(urlPath);
   } catch {
      return { status: 400, path: null };
   }

   const path = normalize(join(rootDir, decoded));
   const rel = relative(rootDir, path);
   if (rel.startsWith("..") || isAbsolute(rel)) {
      return { status: 403, path: null };
   }

   return { status: 200, path };
}
