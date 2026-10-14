import { execFileSync } from "node:child_process";
import { join } from "node:path";

const repoRoot = join(import.meta.dirname, "..", "..");

export function buildPackages() {
  execFileSync("pnpm", ["build"], { cwd: repoRoot, stdio: "inherit" });
}
