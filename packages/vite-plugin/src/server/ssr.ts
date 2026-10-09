import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import type { BundleChunk } from "./assets";
import { materializeServerBundle } from "./facts";
import type { ServerFacts } from "./facts";

export function writeServerBundle(outDir: string, htmlChunks: readonly BundleChunk[], facts: ServerFacts): void {
  rmSync(outDir, { recursive: true, force: true });
  const entry = materializeServerBundle(htmlChunks, outDir, facts);
  writeFileSync(join(outDir, "entry.js"), `export { discover, handler, prerender } from "./${entry}";\n`);
}
