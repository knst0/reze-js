import { execFile, execFileSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { promisify } from "node:util";

const run = promisify(execFile);
const repoRoot = join(import.meta.dirname, "..", "..");

export function buildPackages() {
  execFileSync("pnpm", ["build"], { cwd: repoRoot, stdio: "inherit" });
}

export function writeSources(dir, files) {
  rmSync(dir, { recursive: true, force: true });
  for (const [file, content] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, file)), { recursive: true });
    writeFileSync(join(dir, file), content);
  }
}

export async function viteBuild(cwd, label, env = {}) {
  try {
    await run("pnpm", ["exec", "vite", "build", "--logLevel", "error"], { cwd, env: { ...process.env, ...env }, maxBuffer: 1 << 26 });
  } catch (error) {
    throw new Error(`${label} build failed:\n${error.stdout}\n${error.stderr}`);
  }
}
