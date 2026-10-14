import { brotliCompressSync, constants, gzipSync } from "node:zlib";

export const sizeFields = ["raw", "gzip", "brotli"];

export function measure(code) {
  const content = Buffer.from(code);
  return {
    raw: content.length,
    gzip: gzipSync(content, { level: 9 }).length,
    brotli: brotliCompressSync(content, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length,
  };
}

export function sumSizes(sizes) {
  const total = { raw: 0, gzip: 0, brotli: 0 };
  for (const size of sizes) {
    for (const field of sizeFields) total[field] += size[field];
  }
  return total;
}

export function diffSizes(a, b) {
  return Object.fromEntries(sizeFields.map((field) => [field, a[field] - b[field]]));
}
