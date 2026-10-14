import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join, relative } from "node:path";

const root = join(import.meta.dirname, "..", "..");
const outDir = join(root, "coverage", "by-file");
const indexPath = join(outDir, "index.json");

function scan(dir, out = []) {
  if (!existsSync(dir)) return out;
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) scan(path, out);
    else if (/\.spec\.tsx?$/.test(entry.name)) out.push(path);
  }
  return out;
}

function specFiles() {
  const files = [];
  for (const pkg of readdirSync(join(root, "packages"), { withFileTypes: true })) {
    if (pkg.isDirectory()) scan(join(root, "packages", pkg.name, "tests"), files);
  }
  scan(join(root, "docs", "plugins"), files);
  return files.map((path) => relative(root, path).replaceAll("\\", "/")).sort((a, b) => a.localeCompare(b));
}

function collect() {
  mkdirSync(outDir, { recursive: true });
  const vp = existsSync(join(root, "node_modules", ".bin", "vp")) ? join(root, "node_modules", ".bin", "vp") : "vp";
  const index = {};
  for (const file of specFiles()) {
    const slug = file.replaceAll("/", "-").replace(/\.spec\.tsx?$/, "");
    const dir = join(outDir, slug);
    console.log(`coverage-map: ${file}`);
    spawnSync(vp, ["test", "run", file, "--coverage", "--coverage.reporter=json", `--coverage.reportsDirectory=coverage/by-file/${slug}`], {
      cwd: root,
      encoding: "utf8",
    });
    const covered = new Set();
    const finalPath = join(dir, "coverage-final.json");
    if (existsSync(finalPath)) {
      const data = JSON.parse(readFileSync(finalPath, "utf8"));
      for (const [path, entry] of Object.entries(data)) {
        const rel = relative(root, path).replaceAll("\\", "/");
        for (const [id, count] of Object.entries(entry.s)) {
          if (count > 0) covered.add(`${rel}:${entry.statementMap[id].start.line}`);
        }
      }
    }
    index[file] = [...covered].sort((a, b) => a.localeCompare(b));
  }
  writeFileSync(indexPath, `${JSON.stringify(index, null, 2)}\n`);
  console.log(`coverage-map: wrote ${indexPath}`);
}

function report() {
  const allowlistPath = process.argv.includes("--allowlist") ? process.argv[process.argv.indexOf("--allowlist") + 1] : undefined;
  const allowed = new Map();
  if (allowlistPath && existsSync(join(root, allowlistPath))) {
    for (const entry of JSON.parse(readFileSync(join(root, allowlistPath), "utf8"))) allowed.set(entry.file, entry.keptBecause);
  }
  const index = JSON.parse(readFileSync(indexPath, "utf8"));
  const files = Object.keys(index);
  const sets = new Map(files.map((file) => [file, new Set(index[file])]));
  const subsets = [];
  const zeroUnique = [];
  for (const file of files) {
    const set = sets.get(file);
    for (const other of files) {
      if (other === file) continue;
      const otherSet = sets.get(other);
      if (set.size < otherSet.size && [...set].every((id) => otherSet.has(id))) {
        subsets.push({ file, subsetOf: other });
        break;
      }
    }
    const others = new Set(files.filter((f) => f !== file).flatMap((f) => index[f]));
    if ([...set].every((id) => others.has(id))) zeroUnique.push(file);
  }
  const shown = (file) => !allowed.has(file);
  console.log("strict coverage subsets:");
  for (const { file, subsetOf } of subsets.filter(({ file }) => shown(file))) console.log(`  ${file} < ${subsetOf}`);
  console.log("zero unique covered statements:");
  for (const file of zeroUnique.filter(shown)) console.log(`  ${file}`);
  for (const [file, reason] of allowed) console.log(`  (allowlisted) ${file}: ${reason}`);
}

if (process.argv[2] === "collect") collect();
else if (process.argv[2] === "report") report();
else {
  console.error("usage: coverage-map.mjs <collect|report> [--allowlist <path>]");
  process.exit(1);
}
