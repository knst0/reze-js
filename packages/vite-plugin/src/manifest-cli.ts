#!/usr/bin/env node
import { writeManifest } from "./manifest";

try {
  const report = writeManifest(process.argv[2] ?? process.cwd());
  for (const { file, reason } of report.skipped) {
    process.stderr.write(`reze-manifest: skipped ${file}: ${reason}\n`);
  }
  process.stdout.write(`reze-manifest: wrote ${report.written} module(s) to ${report.path}\n`);
} catch (error) {
  process.stderr.write(`reze-manifest: ${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = 1;
}
