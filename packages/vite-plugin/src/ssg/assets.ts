import { canonicalModuleId } from "../module-identity";

export interface BundleAsset {
  type: "asset";
  fileName: string;
  originalFileName?: string | undefined;
  originalFileNames?: string[] | undefined;
}

export interface BundleChunk {
  type: "chunk";
  fileName: string;
  code?: string;
  isEntry: boolean;
  facadeModuleId?: string | null | undefined;
  moduleIds: readonly string[];
  imports: readonly string[];
  dynamicImports: readonly string[];
  viteMetadata?: { importedAssets?: Set<string> | readonly string[] | undefined; importedCss?: Set<string> | readonly string[] | undefined } | undefined;
}

export type BundleOutput = BundleAsset | BundleChunk;

export interface ClientRegistry {
  assetFile(id: string): string;
  assetEntries(): { id: string; file: string }[];
  entryChunk(bootstrapId: string): BundleChunk;
  chunkFileForModule(absFile: string): string | undefined;
  staticClosure(seedChunks: readonly string[]): { js: string[]; css: string[] };
}
export interface ClientAssetInputs {
  files: Map<string, { id: string; postfix: string }>;
  inlined: Map<string, string>;
  publicFiles: Map<string, string>;
}

export function buildClientRegistry(outputs: readonly BundleOutput[], root: string, inputs?: ClientAssetInputs): ClientRegistry {
  const assets = new Map<string, string>();
  const ambiguous = new Set<string>();
  const chunks: BundleChunk[] = [];
  for (const output of outputs) {
    if (output.type === "chunk") {
      chunks.push(output);
      continue;
    }
    for (const original of output.originalFileNames ?? (output.originalFileName === undefined ? [] : [output.originalFileName])) {
      const id = canonicalModuleId(original, root);
      if (assets.has(id) && assets.get(id) !== output.fileName) ambiguous.add(id);
      assets.set(id, output.fileName);
    }
  }
  const assetFile = (id: string): string => {
    const key = canonicalModuleId(id, root);
    if (ambiguous.has(key)) throw new Error(`[reze] asset ${JSON.stringify(key)} has ambiguous client outputs`);
    const file = assets.get(key);
    if (file === undefined) throw new Error(`[reze] asset ${JSON.stringify(key)} has no client output to link against`);
    return file;
  };
  const canonical: { id: string; file: string }[] = [];
  if (inputs === undefined) {
    for (const id of assets.keys()) canonical.push({ id, file: assetFile(id) });
  } else {
    for (const [id, source] of inputs.files) {
      const file = inputs.inlined.get(id) ?? inputs.publicFiles.get(id) ?? encodeURI(assetFile(source.id)) + source.postfix;
      canonical.push({ id, file });
    }
  }
  const chunkByFile = new Map<string, BundleChunk>();
  const chunkByModule = new Map<string, BundleChunk>();
  for (const chunk of chunks) {
    chunkByFile.set(chunk.fileName, chunk);
    for (const id of chunk.moduleIds) chunkByModule.set(id.replace(/\\/g, "/"), chunk);
  }
  return {
    assetFile,
    assetEntries(): { id: string; file: string }[] {
      return [...canonical];
    },
    entryChunk(bootstrapId: string): BundleChunk {
      let entry: BundleChunk | undefined;
      for (const candidate of chunks) {
        if (!candidate.isEntry) continue;
        const queue = [candidate];
        const seen = new Set<string>();
        while (queue.length > 0) {
          const chunk = queue.pop()!;
          if (seen.has(chunk.fileName)) continue;
          seen.add(chunk.fileName);
          if (chunk.facadeModuleId === bootstrapId || chunk.moduleIds.includes(bootstrapId)) {
            if (entry !== undefined) throw new Error("[reze] client bootstrap belongs to multiple entry chunks");
            entry = candidate;
            break;
          }
          for (const file of chunk.imports) {
            const dependency = chunkByFile.get(file);
            if (dependency !== undefined) queue.push(dependency);
          }
        }
      }
      if (entry === undefined) throw new Error("[reze] client build produced no entry chunk for the SSG bootstrap");
      return entry;
    },
    chunkFileForModule(absFile: string): string | undefined {
      return chunkByModule.get(absFile.replace(/\\/g, "/"))?.fileName;
    },
    staticClosure(seedChunks: readonly string[]): { js: string[]; css: string[] } {
      const js: string[] = [];
      const css: string[] = [];
      const seenJs = new Set<string>();
      const seenCss = new Set<string>();
      const queue = [...seedChunks];
      while (queue.length > 0) {
        const file = queue.shift()!;
        if (seenJs.has(file)) continue;
        seenJs.add(file);
        const chunk = chunkByFile.get(file);
        if (chunk === undefined) continue;
        js.push(chunk.fileName);
        for (const cssFile of chunk.viteMetadata?.importedCss ?? []) {
          if (seenCss.has(cssFile)) continue;
          seenCss.add(cssFile);
          css.push(cssFile);
        }
        queue.push(...chunk.imports);
      }
      return { js, css };
    },
  };
}
