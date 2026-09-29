import { withCodSpeed } from "@codspeed/tinybench-plugin";
import { buildPaths, createMemoryHistory, createRouter, type OutputMatch, type RouteDefinition } from "@rezejs/router";
import { routesDts, routesModule, scanRoutes } from "@rezejs/router/fs";
import { Bench } from "tinybench";

const GROUP_COUNT = 40;
const MATCH_REPEATS = 50;
const MISS_REPEATS = 20;
const HREF_REPEATS = 300;
const HISTORY_PUSHES = 1500;
const HISTORY_BACK = 200;
const HISTORY_EXTRA_PUSHES = 300;
const ROUTES_DIR = "/app/src/routes";
const DTS_FILE = "/app/src/routes.gen.d.ts";
const HREF_BASE = "/app";

let sink = 0;

function routeDefinitions(): RouteDefinition[] {
  const defs: RouteDefinition[] = [];
  for (let i = 0; i < GROUP_COUNT; i++) {
    defs.push({ path: `/s${i}` });
    defs.push({ path: `/u${i}/:uid${i}` });
    defs.push({ path: `/t${i}`, children: [{ path: "/a" }, { path: `/:tid${i}` }] });
    defs.push({ path: `/w${i}/*p${i}` });
    defs.push({ path: `/o${i}/:pg${i}?` });
  }
  return defs;
}

function consume(matches: OutputMatch[]): void {
  sink += matches.length;
  for (const match of matches) for (const value of Object.values(match.params)) sink += value.length;
}

const defs = routeDefinitions();
const router = createRouter({ routes: defs, history: createMemoryHistory("/") });

const staticUrls: string[] = [];
const paramUrls: string[] = [];
const nestedUrls: string[] = [];
const missUrls: string[] = [];
for (let i = 0; i < GROUP_COUNT; i++) {
  staticUrls.push(`/s${i}`);
  paramUrls.push(`/u${i}/a%20b`, `/u${i}/x%2Fy`, `/u${i}/%E0%A4%A`);
  nestedUrls.push(`/t${i}/a`, `/t${i}/v${i}`, `/w${i}/a/b/c`, `/o${i}`, `/o${i}/3`);
  missUrls.push(`/missing-${i}/zzz`);
}

const staticSweep = Array.from({ length: MATCH_REPEATS }, (_, k) => staticUrls[k % staticUrls.length]);
const paramSweep = Array.from({ length: MATCH_REPEATS }, (_, k) => paramUrls[k % paramUrls.length]);
const nestedSweep = Array.from({ length: MATCH_REPEATS }, (_, k) => nestedUrls[k % nestedUrls.length]);
const missSweep = Array.from({ length: MISS_REPEATS }, (_, k) => missUrls[k % missUrls.length]);
const mixedSweep = [...staticSweep.slice(0, 20), ...paramSweep.slice(0, 10), ...nestedSweep.slice(0, 19), missSweep[0]];

const bench = withCodSpeed(new Bench());

bench.add("match: static hits", () => {
  for (const url of staticSweep) consume(router.match(url));
});

bench.add("match: param hits with encoded values", () => {
  for (const url of paramSweep) consume(router.match(url));
});

bench.add("match: nested, optional and wildcard hits", () => {
  for (const url of nestedSweep) consume(router.match(url));
});

bench.add("match: misses across the table", () => {
  for (const url of missSweep) consume(router.match(url));
});

bench.add("match: mixed sweep of hits and a miss", () => {
  for (const url of mixedSweep) consume(router.match(url));
});

{
  const paths = buildPaths(defs);
  const statics = Array.from({ length: GROUP_COUNT }, (_, i) => paths[`s${i}`] as (search?: unknown, hash?: unknown) => string);
  const params = Array.from(
    { length: GROUP_COUNT },
    (_, i) => paths[`byUid${i}`] as (value: string | number, search?: unknown, hash?: unknown) => string,
  );
  const nested = Array.from(
    { length: GROUP_COUNT },
    (_, i) => (paths[`t${i}`] as Record<string, (value: string | number) => string>)[`byTid${i}`],
  );
  const splats = Array.from(
    { length: GROUP_COUNT },
    (_, i) => paths[`byP${i}`] as (value: string | string[], search?: unknown, hash?: unknown) => string,
  );
  const optionals = Array.from(
    { length: GROUP_COUNT },
    (_, i) => paths[`byPg${i}`] as (value?: string | number, search?: unknown, hash?: unknown) => string,
  );
  bench.add("paths: build hrefs through builders", () => {
    for (let k = 0; k < HREF_REPEATS; k++) {
      const index = k % GROUP_COUNT;
      const kind = k % 5;
      if (kind === 0) sink += statics[index]({ q: "x", n: 2 }, "sec").length;
      else if (kind === 1) sink += params[index]("a/b", { q: "x" }, "sec").length;
      else if (kind === 2) sink += nested[index](7).length;
      else if (kind === 3) sink += splats[index](index % 2 === 0 ? ["a", "b"] : "a/b", { q: "x" }, "sec").length;
      else if (index % 2 === 0) sink += optionals[index]().length;
      else sink += optionals[index](3, { q: "x" }, "sec").length;
    }
  });
}

bench.add("paths: build the table", () => {
  sink += Object.keys(buildPaths(defs)).length;
});

bench.add("history: push, traverse and replace", () => {
  const history = createMemoryHistory("/");
  const stop = history.listen(() => {
    sink += 1;
  });
  for (let i = 0; i < HISTORY_PUSHES; i++) history.push(`/p${i}`, undefined);
  history.go(-HISTORY_BACK);
  for (let i = 0; i < HISTORY_EXTRA_PUSHES; i++) history.push(`/q${i}`, undefined);
  history.replace("/r", undefined);
  sink += history.get().index;
  stop();
});

const routeFiles: string[] = [];
for (let i = 0; i < GROUP_COUNT; i++) {
  routeFiles.push(`s${i}.tsx`, `u${i}/[id].tsx`, `u${i}/index.tsx`, `w${i}/[...path].tsx`, `o${i}/[[page]].tsx`, `(g${i})/promo${i}.tsx`);
}
const scannedRoutes = scanRoutes(routeFiles);

bench.add("fs: scan route files", () => {
  sink += scanRoutes(routeFiles).length;
});

bench.add("fs: routes module codegen", () => {
  sink += routesModule(scannedRoutes, ROUTES_DIR, HREF_BASE).length;
});

bench.add("fs: routes dts codegen", () => {
  sink += routesDts(scannedRoutes, HREF_BASE, DTS_FILE, ROUTES_DIR).length;
});

bench.add("fs: scan plus module plus dts", () => {
  const routes = scanRoutes(routeFiles);
  sink += routesModule(routes, ROUTES_DIR, HREF_BASE).length + routesDts(routes, HREF_BASE, DTS_FILE, ROUTES_DIR).length;
});

await bench.run();
console.table(bench.table());
