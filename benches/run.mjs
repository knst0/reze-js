import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { parseArgs } from "node:util";

import { buildPackages } from "./kit/build.mjs";
import { selectNames } from "./kit/cli.mjs";

const root = join(import.meta.dirname, "..");

const suites = {
  "micro-signals": { kind: "micro", pkg: "signals" },
  "micro-dom": { kind: "micro", pkg: "dom" },
  "micro-router": { kind: "micro", pkg: "router" },
  accessor: { kind: "bundled", file: "accessor" },
  "fixed-render": { kind: "bundled", file: "fixed-render" },
  rust: { kind: "rust" },
};
const names = Object.keys(suites);

const { values: args } = parseArgs({
  options: {
    suite: { type: "string" },
    "skip-build": { type: "boolean", default: false },
  },
});

let selected;
try {
  selected = args.suite === undefined ? names.filter((name) => name !== "rust") : selectNames(names, args.suite, "suite");
} catch (error) {
  console.error(error.message);
  process.exit(1);
}

if (selected.some((name) => suites[name].kind === "micro") && !args["skip-build"]) {
  buildPackages();
}

function spawn(command, commandArgs) {
  const result = spawnSync(command, commandArgs, { cwd: root, stdio: "inherit" });
  return result.status ?? 1;
}

for (const name of selected) {
  const suite = suites[name];
  console.log(`\n=== bench: ${name} ===`);
  let code = 0;
  if (suite.kind === "micro") {
    code = spawn(process.execPath, [
      "--import",
      join(root, "benches", "kit", "production-env.mjs"),
      join(root, "packages", suite.pkg, "benches", "codspeed.ts"),
    ]);
  } else if (suite.kind === "bundled") {
    code = spawn(process.execPath, [
      join(root, "benches", "kit", "bundled.mjs"),
      join(root, "packages", "signals", "benches", `${suite.file}.ts`),
    ]);
  } else if (suite.kind === "rust") {
    code = spawn("cargo", ["bench", "-p", "reze_compiler"]);
  }
  if (code !== 0) {
    console.error(`bench: suite "${name}" failed with exit code ${code}`);
    process.exit(code);
  }
}
