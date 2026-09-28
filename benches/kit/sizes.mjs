import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";

export const brotliSize = (buf) => brotliCompressSync(buf, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length;

export function assetSizes(dist) {
  const sizes = new Map();
  for (const file of readdirSync(join(dist, "assets"))) {
    if (!file.endsWith(".js")) continue;
    const content = readFileSync(join(dist, "assets", file));
    sizes.set(`/assets/${file}`, { raw: content.length, gzip: gzipSync(content, { level: 9 }).length, brotli: brotliSize(content) });
  }
  return sizes;
}

export function sumSizes(sizes, paths) {
  const total = { raw: 0, gzip: 0, brotli: 0 };
  for (const path of paths) {
    const size = sizes.get(path);
    if (size === undefined) throw new Error(`loaded script ${path} is not a built asset`);
    total.raw += size.raw;
    total.gzip += size.gzip;
    total.brotli += size.brotli;
  }
  return total;
}

export const diffSizes = (a, b) => ({ raw: a.raw - b.raw, gzip: a.gzip - b.gzip, brotli: a.brotli - b.brotli });

export function totalSize(dist) {
  const sizes = assetSizes(dist);
  return sumSizes(sizes, sizes.keys());
}
