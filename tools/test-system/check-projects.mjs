import { spawnSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";

const root = join(import.meta.dirname, "..", "..");

function scan(dir, out = []) {
  if (!existsSync(dir)) return out;
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) scan(path, out);
    else if (/\.spec\.tsx?$/.test(entry.name)) out.push(path);
  }
  return out;
}

const scanned = [];
for (const pkg of readdirSync(join(root, "packages"), { withFileTypes: true })) {
  if (pkg.isDirectory()) scan(join(root, "packages", pkg.name, "tests"), scanned);
}
scan(join(root, "docs", "plugins"), scanned);

const vp = existsSync(join(root, "node_modules", ".bin", "vp")) ? join(root, "node_modules", ".bin", "vp") : "vp";
const result = spawnSync(vp, ["test", "list"], { cwd: root, encoding: "utf8" });
if (result.status !== 0) {
  process.stderr.write(result.stdout ?? "");
  process.stderr.write(result.stderr ?? "");
  console.error("check-projects: `vp test list` failed");
  process.exit(1);
}

const listed = new Set();
for (const line of (result.stdout ?? "").split("\n")) {
  for (const match of line.matchAll(/[\w@./-]+\.spec\.tsx?/g)) listed.add(match[0].replaceAll("\\", "/"));
}

const orphans = scanned
  .map((path) => relative(root, path).replaceAll("\\", "/"))
  .filter((file) => ![...listed].some((token) => file === token || file.endsWith(`/${token}`) || token.endsWith(`/${file}`)));

if (orphans.length > 0) {
  console.error("check-projects: spec files not covered by any Vitest project:");
  for (const file of orphans) console.error(`  ${file}`);
  process.exit(1);
}
console.log(`check-projects: ${scanned.length} spec files, all covered by a Vitest project`);
