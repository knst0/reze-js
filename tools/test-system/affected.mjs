import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..", "..");
const since = process.argv[2];

function git(args) {
  return execFileSync("git", args, { cwd: root, encoding: "utf8" }).split("\n").filter(Boolean);
}

const changed = new Set(git(["diff", "--name-only", since ?? "HEAD"]));
for (const file of git(["ls-files", "--others", "--exclude-standard"])) changed.add(file);

function projectNames() {
  const byPackage = new Map();
  const add = (pkg, name) => {
    if (!byPackage.has(pkg)) byPackage.set(pkg, []);
    byPackage.get(pkg).push(name);
  };
  for (const pkg of readdirSync(join(root, "packages"))) {
    const dir = join(root, "packages", pkg);
    if (!existsSync(dir)) continue;
    for (const file of readdirSync(dir)) {
      if (!/^vitest(\.node)?\.config\.mts$/.test(file)) continue;
      const text = readFileSync(join(dir, file), "utf8");
      for (const match of text.matchAll(/domProject\("([^"]+)"/g)) add(pkg, match[1]);
      for (const match of text.matchAll(/nodeProject\("([^"]+)"/g)) add(pkg, `${match[1]} (node)`);
      for (const match of text.matchAll(/\bname:\s*"([^"]+)"/g)) add(pkg, match[1]);
    }
  }
  const docs = readFileSync(join(root, "docs", "vitest.config.mts"), "utf8");
  for (const match of docs.matchAll(/\bname:\s*"([^"]+)"/g)) add("docs", match[1]);
  return byPackage;
}

function dependents() {
  const reverse = new Map();
  const dirByName = new Map();
  for (const pkg of readdirSync(join(root, "packages"))) {
    const manifestPath = join(root, "packages", pkg, "package.json");
    if (!existsSync(manifestPath)) continue;
    dirByName.set(JSON.parse(readFileSync(manifestPath, "utf8")).name, pkg);
  }
  for (const pkg of readdirSync(join(root, "packages"))) {
    const manifestPath = join(root, "packages", pkg, "package.json");
    if (!existsSync(manifestPath)) continue;
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    const deps = { ...manifest.dependencies, ...manifest.devDependencies };
    for (const dep of Object.keys(deps)) {
      const depPkg = dirByName.get(dep);
      if (depPkg === undefined || depPkg === pkg) continue;
      if (!reverse.has(depPkg)) reverse.set(depPkg, new Set());
      reverse.get(depPkg).add(pkg);
    }
  }
  return reverse;
}

const byPackage = projectNames();
const allProjects = [...byPackage.values()].flat();
const reverse = dependents();

const selectedPackages = new Set();
let runAll = false;
for (const file of changed) {
  if (
    file === "vitest.config.mts" ||
    file === "vitest.shared.ts" ||
    file === "package.json" ||
    file === "pnpm-workspace.yaml" ||
    file.startsWith("tools/test-system/")
  ) {
    runAll = true;
    continue;
  }
  const pkgMatch = /^packages\/([^/]+)\//.exec(file);
  if (pkgMatch) {
    selectedPackages.add(pkgMatch[1]);
    continue;
  }
  if (file.startsWith("docs/")) selectedPackages.add("docs");
}

const queue = [...selectedPackages];
while (queue.length > 0) {
  const pkg = queue.pop();
  for (const dependent of reverse.get(pkg) ?? []) {
    if (!selectedPackages.has(dependent)) {
      selectedPackages.add(dependent);
      queue.push(dependent);
    }
  }
}
const projects = runAll ? allProjects : [...selectedPackages].flatMap((pkg) => byPackage.get(pkg) ?? []);

if (projects.length === 0) {
  console.log("affected: no test projects affected by the changed files");
  process.exit(0);
}

console.log(`affected: running projects ${projects.join(", ")}`);
const vp = existsSync(join(root, "node_modules", ".bin", "vp")) ? join(root, "node_modules", ".bin", "vp") : "vp";
const args = ["test", "run", ...projects.flatMap((name) => ["--project", name])];
const result = spawnSync(vp, args, { cwd: root, stdio: "inherit" });
process.exit(result.status ?? 1);
