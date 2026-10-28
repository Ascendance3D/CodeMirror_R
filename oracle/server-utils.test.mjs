import { createServer, request } from "node:http";
import { mkdtemp, rm, writeFile, mkdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import assert from "node:assert";

async function runTest() {
  const { safeResolve } = await import("./server-utils.mjs");
  const root = await mkdtemp(join(tmpdir(), "server-test-"));
  await writeFile(join(root, "test.js"), "console.log('test');");

  const sibling = root + "-evil";
  await mkdir(sibling);
  await writeFile(join(sibling, "evil.js"), "console.log('evil');");

  const server = createServer(async (req, res) => {
      const url = new URL(req.url, "http://x");
      const s = safeResolve(url.pathname, root);
      if (s.status !== 200) { res.writeHead(s.status); return res.end(); }

      try {
        const { readFile } = await import("node:fs/promises");
        const body = await readFile(s.path);
        res.writeHead(200);
        res.end(body);
      } catch {
        res.writeHead(404);
        res.end();
      }
  });
  await new Promise(r => server.listen(0, "127.0.0.1", r));
  const port = server.address().port;

  const fetchNet = (path) => new Promise(resolve => {
      const req = request({
          hostname: '127.0.0.1',
          port,
          path,
          method: 'GET'
      }, (res) => {
          res.on("data", () => {});
          res.on("end", () => resolve(res.statusCode));
      });
      req.end();
  });

  assert.strictEqual(await fetchNet("/test.js"), 200, "200 for valid path");
  assert.strictEqual(await fetchNet("/missing.js"), 404, "404 for missing file");

  assert.strictEqual(await fetchNet("/%2e%2e%2fmissing.js"), 403, "403 for traversal attempt");
  const siblingName = sibling.split('/').pop();
  assert.strictEqual(await fetchNet(`/%2e%2e%2f${siblingName}%2fevil.js`), 403, "403 for sibling prefix");
  assert.strictEqual(await fetchNet("/%ZZ"), 400, "400 for malformed URI");

  server.close();
  await rm(root, {recursive: true});
  await rm(sibling, {recursive: true});
}

runTest().catch(e => { console.error(e); process.exit(1); });
