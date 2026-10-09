import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

import { workerEntrySource } from "./adapter";
import type { BundleChunk } from "./assets";
import type { AppMode } from "./export-graph";
import { materializeServerBundle } from "./facts";
import type { ServerFacts } from "./facts";
import type { ResolvedSsgOptions } from "./options";
import { resolvePathsCallbacks } from "./options";
import { buildRedirectPage, rebaseParts } from "./template";
import type { TemplateParts } from "./template";
import { canonicalPageUrl, joinBase, normalizePageUrl, outputFileFor, pageDepth, planOutputs } from "./urls";
import { createTempDir, removeTempDir, runWorker } from "./workers";

export type RenderedPage = { status: "render"; html: string } | { status: "redirect"; to: string; replace: boolean };

interface PrerenderOutput {
  mode: AppMode["kind"];
  pages: { pathname: string; result: RenderedPage }[];
}

export interface SsgBuildInput {
  root: string;
  base: string;
  outDir: string;
  publicDir: string;
  mode: AppMode;
  options: ResolvedSsgOptions;
  htmlChunks: readonly BundleChunk[];
  facts: ServerFacts;
  template: TemplateParts;
  redirectFile: string;
}

export async function prerenderSite(input: SsgBuildInput): Promise<void> {
  const { options } = input;
  const paths = await resolvePathsCallbacks(options.paths);
  const tempDir = createTempDir(input.root);
  try {
    const entry = materializeServerBundle(input.htmlChunks, tempDir, input.facts);
    const workerFile = join(tempDir, "reze-ssg-worker.mjs");
    writeFileSync(workerFile, workerEntrySource(`./${entry}`));
    const rendered = await runWorker<PrerenderOutput>(workerFile, { paths, trailingSlash: options.trailingSlash }, options.timeoutMs);
    if (rendered.mode !== input.mode.kind) {
      throw new Error(`[reze] SSG entry mode changed between analysis (${input.mode.kind}) and discovery (${rendered.mode})`);
    }
    const urls = rendered.pages.map((page) => page.pathname);
    checkPublicCollisions(input.publicDir, [...planOutputs(urls).keys()]);
    const redirects = new Map<string, string>();
    for (const page of rendered.pages) {
      if (page.result.status === "redirect") {
        const target = redirectPageTarget(page.result.to, input.base, options);
        if (target !== undefined) redirects.set(page.pathname, target);
      }
      writePage(input, page.pathname, page.result, urls);
    }
    assertNoRedirectCycles(redirects);
  } finally {
    removeTempDir(tempDir);
  }
}

function writePage(input: SsgBuildInput, pathname: string, page: RenderedPage, concrete: readonly string[]): void {
  const outFile = join(input.outDir, outputFileFor(pathname));
  mkdirSync(dirname(outFile), { recursive: true });
  if (page.status === "render") {
    writeFileSync(outFile, page.html);
    return;
  }
  const target = redirectPageTarget(page.to, input.base, input.options);
  if (target !== undefined && !concrete.includes(target)) {
    throw new Error(`[reze] redirect target ${JSON.stringify(page.to)} for ${JSON.stringify(pathname)} is not a generated SSG page`);
  }
  const depth = pageDepth(pathname);
  const assetOrigin = /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(input.base) || input.base.startsWith("//");
  const suffix = new URL(page.to, "https://reze.invalid/");
  const destination =
    target === undefined ? page.to : joinBase(assetOrigin ? "/" : input.base, target.slice(1), depth) + suffix.search + suffix.hash;
  writeFileSync(
    outFile,
    buildRedirectPage({
      parts: rebaseParts(input.template, depth),
      to: destination,
      replace: page.replace,
      redirectSrc: joinBase(input.base, input.redirectFile, depth),
    }),
  );
}

function checkPublicCollisions(publicDir: string, files: readonly string[]): void {
  if (!existsSync(publicDir)) return;
  const seen: string[] = [];
  const walk = (current: string, prefix: string): void => {
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const rel = prefix === "" ? entry.name : `${prefix}/${entry.name}`;
      if (entry.isDirectory()) walk(join(current, entry.name), rel);
      else seen.push(rel);
    }
  };
  walk(publicDir, "");
  for (const file of files) {
    if (seen.includes(file)) {
      throw new Error(`[reze] SSG output ${JSON.stringify(file)} collides with a public directory file`);
    }
  }
}

function redirectPageTarget(to: string, base: string, options: ResolvedSsgOptions): string | undefined {
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(to) || to.startsWith("//")) {
    if (!/^https?:/i.test(to) && !to.startsWith("//")) throw new Error(`[reze] unsupported redirect target ${JSON.stringify(to)}`);
    return undefined;
  }
  let pathname = new URL(to, "https://reze.invalid/").pathname;
  const prefix = base.startsWith("/") && !base.startsWith("//") ? base.replace(/\/$/, "") : "";
  if (prefix !== "") {
    if (pathname === prefix) pathname = "/";
    else if (pathname.startsWith(`${prefix}/`)) pathname = pathname.slice(prefix.length);
    else throw new Error(`[reze] redirect target ${JSON.stringify(to)} is outside the SSG base`);
  }
  return canonicalPageUrl(normalizePageUrl(pathname), options.trailingSlash);
}

function assertNoRedirectCycles(redirects: ReadonlyMap<string, string>): void {
  const states = new Map<string, 1 | 2>();
  for (const start of redirects.keys()) {
    if (states.has(start)) continue;
    const path: string[] = [];
    let current: string | undefined = start;
    while (current !== undefined && states.get(current) !== 2) {
      if (states.get(current) === 1) throw new Error(`[reze] redirect cycle: ${[...path, current].join(" -> ")}`);
      states.set(current, 1);
      path.push(current);
      current = redirects.get(current);
    }
    for (const node of path) states.set(node, 2);
  }
}
