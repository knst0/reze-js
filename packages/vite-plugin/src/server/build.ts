import { ClientId } from "./adapter";
import type { BundleChunk, BundleOutput, ClientAssetInputs } from "./assets";
import { buildClientRegistry } from "./assets";
import type { AppMode } from "./export-graph";
import { clientBuildId, createServerFacts, writeAssetManifest } from "./facts";
import type { ResolvedSsgOptions } from "./options";
import type { ResolvedSsrOptions } from "./options";
import { prerenderSite } from "./ssg";
import { writeServerBundle } from "./ssr";
import { bootstrapScriptSrc, countRootIds, prepareTemplate } from "./template";

export interface CapturedState {
  outputs: BundleOutput[];
  templateHtml: string;
  templateFile: string;
  redirectFile: string;
  htmlChunks: BundleChunk[];
}

export interface ServerBuildInput {
  root: string;
  base: string;
  clientOutDir: string;
  publicDir: string;
  mode: AppMode;
  captured: CapturedState;
  assets: ClientAssetInputs;
  moduleFiles: ReadonlyMap<string, string>;
  ssg: ResolvedSsgOptions | undefined;
  ssr: ResolvedSsrOptions | undefined;
}

export async function runServerBuild(input: ServerBuildInput): Promise<void> {
  const { captured, ssg, ssr } = input;
  const primary = ssg ?? ssr;
  if (primary === undefined) throw new Error("[reze] server build needs ssg or ssr options");
  const { rootId } = primary;
  const registry = buildClientRegistry(captured.outputs, input.root, input.assets);
  const bootstrapFile = registry.entryChunk(ClientId).fileName;
  bootstrapScriptSrc(captured.templateHtml, bootstrapFile);
  if (countRootIds(captured.templateHtml, rootId) !== 1) {
    throw new Error(`[reze] built template must contain exactly one element with id ${JSON.stringify(rootId)}`);
  }
  const prepared = prepareTemplate(captured.templateHtml, rootId);
  const clientFiles = captured.outputs.map((output) => output.fileName).filter((file) => file !== captured.templateFile);
  const buildId = clientBuildId(clientFiles);
  const facts = createServerFacts({
    buildId,
    base: input.base,
    rootId,
    timeoutMs: (ssr ?? primary).timeoutMs,
    template: prepared.parts,
    headDefaults: prepared.headDefaults,
    registry,
    moduleFiles: input.moduleFiles,
  });
  writeAssetManifest(input.clientOutDir, input.base, buildId, clientFiles);
  if (ssr !== undefined) writeServerBundle(ssr.outDir, captured.htmlChunks, facts);
  if (ssg !== undefined) {
    await prerenderSite({
      root: input.root,
      base: input.base,
      outDir: input.clientOutDir,
      publicDir: input.publicDir,
      mode: input.mode,
      options: ssg,
      htmlChunks: captured.htmlChunks,
      facts,
      template: prepared.parts,
      redirectFile: captured.redirectFile,
    });
  }
}
