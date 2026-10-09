import { createHash } from "node:crypto";
import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { ServerEntryId } from "./adapter";
import type { BundleChunk, ClientRegistry } from "./assets";
import type { TemplateHead, TemplateParts } from "./template";
import { joinBase } from "./urls";
import { materializeBundle } from "./workers";

export interface ModuleFacts {
  file: string;
  css: string[];
  preload: string[];
}

export interface ServerFacts {
  buildId: string;
  base: string;
  rootId: string;
  timeoutMs: number;
  template: TemplateParts;
  headDefaults: TemplateHead;
  assets: Record<string, string>;
  modules: Record<string, ModuleFacts>;
}

export interface FactsInput {
  buildId: string;
  base: string;
  rootId: string;
  timeoutMs: number;
  template: TemplateParts;
  headDefaults: TemplateHead;
  registry: ClientRegistry;
  moduleFiles: ReadonlyMap<string, string>;
}

export function clientBuildId(files: readonly string[]): string {
  return createHash("sha256")
    .update([...files].sort().join("\n"))
    .digest("hex")
    .slice(0, 16);
}

export function createServerFacts(input: FactsInput): ServerFacts {
  const modules = Object.fromEntries(
    [...input.moduleFiles].map(([moduleId, file]) => {
      const chunk = input.registry.chunkFileForModule(file);
      if (chunk === undefined) throw new Error(`[reze] no client chunk for module ${moduleId}`);
      const closure = input.registry.staticClosure([chunk]);
      return [moduleId, { file: chunk, css: closure.css, preload: closure.js }];
    }),
  );
  return {
    buildId: input.buildId,
    base: input.base,
    rootId: input.rootId,
    timeoutMs: input.timeoutMs,
    template: input.template,
    headDefaults: input.headDefaults,
    assets: Object.fromEntries(input.registry.assetEntries().map((entry) => [entry.id, entry.file])),
    modules,
  };
}

export function factsSource(facts: ServerFacts): string {
  return `export default ${JSON.stringify(facts, null, 2)};\n`;
}

export function materializeServerBundle(htmlChunks: readonly BundleChunk[], dir: string, facts: ServerFacts): string {
  const entry = htmlChunks.find((chunk) => chunk.facadeModuleId === ServerEntryId);
  if (entry === undefined) throw new Error("[reze] HTML bundle produced no server entry chunk");
  materializeBundle(htmlChunks, dir);
  writeFileSync(join(dir, "reze-client.js"), factsSource(facts));
  return entry.fileName;
}

export function writeAssetManifest(clientOutDir: string, base: string, buildId: string, files: readonly string[]): void {
  const manifest = { buildId, files: files.map((file) => joinBase(base, file, 0)) };
  writeFileSync(join(clientOutDir, "reze-assets.json"), `${JSON.stringify(manifest, null, 2)}\n`);
}
