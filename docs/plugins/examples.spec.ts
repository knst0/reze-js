import { execFile } from "node:child_process";
import { mkdir, rm, writeFile } from "node:fs/promises";
import { dirname, join, relative } from "node:path";
import { promisify } from "node:util";

import { compile, type CompileTarget } from "@rezejs/compiler";
import { DEFAULT_ROUTE_EXTENSIONS, routesDts, scanRoutes } from "@rezejs/router/fs";
import { expect, test } from "vitest";

import { collectExamples, readPages, type Example } from "./pages";

const docsDir = join(import.meta.dirname, "..");
const workspaceDir = join(docsDir, "..");
const projectsDir = join(docsDir, "node_modules/.cache/examples");
const examples = (await readPages(join(docsDir, "src/routes"))).flatMap(collectExamples);
const targets: CompileTarget[] = ["client", "island", "html"];
const runFile = promisify(execFile);

const genericRoutes = `declare module "virtual:reze-routes" {
  import type { PathsTree, RouteDefinition } from "@rezejs/router";
  export const routes: readonly RouteDefinition[];
  export const paths: PathsTree;
}
`;
const buildConstants = "declare const __REZE_HTML__: boolean;\n";

test.each(examples.flatMap((example) => targets.map((target) => ({ ...example, target }))))(
  "$id compiles for $target with diagnostics $expectedCodes",
  ({ file, source, expectedCodes, target }) => {
    const { diagnostics } = compile(source, file, { target, moduleId: file, sourceMap: false });
    const reported = diagnostics.map((diagnostic) => diagnostic.code).sort();
    expect(reported, diagnostics.map((diagnostic) => diagnostic.rendered).join("\n")).toEqual(expectedCodes);
  },
);

async function writePageProject(page: string, pageExamples: Example[]): Promise<string> {
  const projectDir = join(projectsDir, page);
  await rm(projectDir, { recursive: true, force: true });
  for (const example of pageExamples) {
    const path = join(projectDir, example.file);
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, `${example.source}\nexport {};\n`);
  }
  const routesDir = join(projectDir, "src/routes");
  const routeFiles = pageExamples.filter((example) => example.file.startsWith("src/routes/"));
  const dtsFile = join(projectDir, "routes.gen.d.ts");
  const routes = scanRoutes(
    routeFiles.map((example) => relative(routesDir, join(projectDir, example.file)).replaceAll("\\", "/")),
    { extensions: DEFAULT_ROUTE_EXTENSIONS },
  );
  await writeFile(dtsFile, routeFiles.length === 0 ? genericRoutes : routesDts(routes, "", dtsFile, routesDir));
  await writeFile(join(projectDir, "build-constants.d.ts"), buildConstants);
  await writeFile(
    join(projectDir, "tsconfig.json"),
    JSON.stringify({
      extends: relative(projectDir, join(workspaceDir, "tsconfig.json")).replaceAll("\\", "/"),
      compilerOptions: { types: ["node", "vite/client"] },
      include: ["**/*.ts", "**/*.tsx"],
      exclude: [],
    }),
  );
  return projectDir;
}

const typedPages = Map.groupBy(
  examples.filter((example) => example.expectedCodes.length === 0),
  (example) => example.page,
);

test.each([...typedPages.keys()])("%s samples typecheck", async (page) => {
  const projectDir = await writePageProject(page, typedPages.get(page) ?? []);
  const tsc = join(workspaceDir, "node_modules/.bin/tsc");
  const output = await runFile(tsc, ["-p", projectDir, "--noEmit", "--pretty", "false"]).then(
    () => "",
    (error: unknown) => (error instanceof Error && "stdout" in error ? String(error.stdout) : String(error)),
  );
  expect(output.trim()).toBe("");
});
