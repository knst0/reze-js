import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { measureDir } from "./bundle.mjs";
import { diffSizes, readSnapshot, recordSnapshot, sizeFields } from "./sizes.mjs";

const pkgDir = process.cwd();
const { name: pkgName } = JSON.parse(readFileSync(join(pkgDir, "package.json"), "utf8"));
const fixturesDir = join(pkgDir, "benches", "size");
const resultsPath = join(pkgDir, "benches", "size.json");
const fixtures = readdirSync(fixturesDir, { withFileTypes: true })
  .filter((entry) => entry.isDirectory())
  .map((entry) => entry.name)
  .sort((a, b) => (a === "baseline" ? -1 : b === "baseline" ? 1 : a.localeCompare(b)));
if (!fixtures.includes("baseline")) throw new Error(`${pkgName} needs benches/size/baseline`);

const sizes = {};
for (const fixture of fixtures) sizes[fixture] = await measureDir(join(fixturesDir, fixture));

const previous = readSnapshot(resultsPath);
const baseline = sizes.baseline;
const format = (value) => value.toLocaleString("en-US");
const signed = (value) => (value > 0 ? `+${format(value)}` : value < 0 ? `-${format(-value)}` : "0");

const rows = fixtures.map((fixture) => {
  const size = sizes[fixture];
  const cost = diffSizes(size, baseline);
  const committed = previous[fixture];
  const row = { feature: fixture };
  for (const field of sizeFields) row[field] = format(size[field]);
  for (const field of sizeFields) row[`cost ${field}`] = fixture === "baseline" ? "—" : signed(cost[field]);
  for (const field of sizeFields) {
    row[`Δ ${field}`] = committed === undefined ? "new" : signed(size[field] - committed[field]);
  }
  return row;
});
console.log(`${pkgName}: JS bytes of production bundles (feature = fixture − baseline)`);
console.table(rows);

recordSnapshot(resultsPath, sizes, { check: process.argv.includes("--check") });
