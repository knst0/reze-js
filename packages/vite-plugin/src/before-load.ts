import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

export const BeforeLoadFlag = "__REZE_BEFORE_LOAD__";

const Mention = /\bbeforeLoad\b/;
const SourceFile = /\.(?:[cm]?[jt]sx?|mdx?|vue|svelte|astro)$/;

export function mentionsBeforeLoad(code: string): boolean {
  return Mention.test(code);
}

export function sourcesMentionBeforeLoad(dir: string, outDir: string, compiles: RegExp): boolean {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === "node_modules" || entry.name.startsWith(".")) continue;
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (path !== outDir && sourcesMentionBeforeLoad(path, outDir, compiles)) return true;
    } else if ((SourceFile.test(entry.name) || compiles.test(entry.name)) && Mention.test(readFileSync(path, "utf8"))) {
      return true;
    }
  }
  return false;
}
