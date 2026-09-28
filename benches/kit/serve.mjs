import { existsSync, readFileSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join } from "node:path";

const mimeTypes = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css" };

export function serve(dist) {
  const server = createServer((req, res) => {
    const path = decodeURIComponent(new URL(req.url, "http://x").pathname);
    const file = join(dist, path);
    const isAsset = path !== "/" && file.startsWith(dist) && existsSync(file) && extname(file) !== "";
    const target = isAsset ? file : join(dist, "index.html");
    res.writeHead(200, {
      "content-type": mimeTypes[extname(target)] ?? "application/octet-stream",
      "cross-origin-opener-policy": "same-origin",
      "cross-origin-embedder-policy": "require-corp",
    });
    res.end(readFileSync(target));
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

export const origin = (server) => `http://127.0.0.1:${server.address().port}`;
