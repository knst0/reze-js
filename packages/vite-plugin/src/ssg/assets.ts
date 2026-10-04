import { statSync } from "node:fs";

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
const MimeByExtension: Record<string, string> = {
  ".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".gif": "image/gif",
  ".svg": "image/svg+xml", ".webp": "image/webp", ".avif": "image/avif",
  ".woff": "font/woff", ".woff2": "font/woff2", ".ttf": "font/ttf", ".otf": "font/otf",
  ".mp4": "video/mp4", ".webm": "video/webm", ".mp3": "audio/mpeg",
};

export function assetMime(file: string): string | undefined {
  const dot = file.lastIndexOf(".");
  return dot < 0 ? undefined : MimeByExtension[file.slice(dot).toLowerCase()];
}

export function decideAssetImport(options: {
  file: string;
  query: string;
  assetsInclude: (id: string) => boolean;
  inlineLimit: number;
}): "raw" | "inline" | "passthrough" | "lookup" {
  if (!options.assetsInclude(options.file)) return "passthrough";
  if (/(?:^|&)raw(?:&|$)/.test(options.query)) return "raw";
  if (/(?:^|&)inline(?:&|$)/.test(options.query)) return "inline";
  let size = -1;
  try {
    size = statSync(options.file).size;
  } catch {
    return "lookup";
  }
  return size >= 0 && size < options.inlineLimit ? "inline" : "lookup";
}

export function buildClientRegistry(outputs: readonly BundleOutput[], root: string): ClientRegistry {
  const normalizedRoot = root.replace(/\\/g, "/").replace(/\/+$/, "");
  const assets = new Map<string, string>();
  const canonical: { id: string; file: string }[] = [];
  const chunks: BundleChunk[] = [];
  for (const output of outputs) {
    if (output.type === "chunk") {
      chunks.push(output);
      continue;
    }
    for (const original of output.originalFileNames ?? (output.originalFileName === undefined ? [] : [output.originalFileName])) {
      const absolute = original.replace(/\\/g, "/");
      const relative = absolute.startsWith(`${normalizedRoot}/`) ? absolute.slice(normalizedRoot.length + 1) : absolute.replace(/^\.\//, "");
      assets.set(relative, output.fileName);
      assets.set(`${normalizedRoot}/${relative}`, output.fileName);
      canonical.push({ id: relative, file: output.fileName });
    }
  }
  const chunkByFile = new Map<string, BundleChunk>();
  const chunkByModule = new Map<string, BundleChunk>();
  for (const chunk of chunks) {
    chunkByFile.set(chunk.fileName, chunk);
    for (const id of chunk.moduleIds) chunkByModule.set(id.replace(/\\/g, "/"), chunk);
  }
  return {
    assetFile(id: string): string {
      const direct = assets.get(id) ?? assets.get(id.replace(/\\/g, "/"));
      if (direct !== undefined) return direct;
      throw new Error(`[reze] asset ${JSON.stringify(id)} has no client output to link against`);
    },
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
