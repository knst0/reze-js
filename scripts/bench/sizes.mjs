import { existsSync, readFileSync, writeFileSync } from "node:fs";
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

const gzipNoiseBytes = 16;
const gzipNoiseRatio = 0.005;

function overBudget(size, recorded) {
  if (recorded === undefined) return true;
  return size.gzip - recorded.gzip > Math.max(gzipNoiseBytes, Math.ceil(recorded.gzip * gzipNoiseRatio));
}

export function readSnapshot(path) {
  return existsSync(path) ? JSON.parse(readFileSync(path, "utf8")).sizes : {};
}

export function recordSnapshot(path, sizes, { check }) {
  const previous = readSnapshot(path);
  const sorted = Object.fromEntries(Object.entries(sizes).sort(([a], [b]) => a.localeCompare(b)));
  if (check) {
    const grown = Object.entries(sorted).filter(([name, size]) => overBudget(size, previous[name]));
    for (const [name, size] of grown) {
      const before = previous[name] ? `${previous[name].gzip}` : "unrecorded";
      console.error(`${name}: gzip ${before} → ${size.gzip}`);
    }
    if (grown.length > 0) {
      console.error(`${path} is over budget; rerun without --check to accept the new sizes`);
      process.exitCode = 1;
    }
    return;
  }
  if (JSON.stringify(sorted) === JSON.stringify(previous)) {
    console.log(`unchanged: ${path}`);
  } else {
    writeFileSync(path, `${JSON.stringify({ schema: 1, recordedAt: new Date().toISOString(), sizes: sorted }, null, 2)}\n`);
    console.log(`wrote ${path}`);
  }
}
