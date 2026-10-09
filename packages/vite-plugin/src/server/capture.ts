import type { BundleOutput } from "./assets";

export interface CapturedOutput {
  output: BundleOutput;
  templateHtml?: string;
}

function readStrings(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((entry): entry is string => typeof entry === "string");
}

function readStringSet(value: unknown): Set<string> | readonly string[] | undefined {
  if (value instanceof Set) {
    const out: string[] = [];
    for (const entry of value) {
      if (typeof entry === "string") out.push(entry);
    }
    return out;
  }
  if (Array.isArray(value)) return readStrings(value);
  return undefined;
}

function decodeSource(source: unknown): string {
  if (typeof source === "string") return source;
  if (source instanceof Uint8Array) return Buffer.from(source).toString("utf8");
  throw new Error("[reze] built template index.html has no readable source");
}

export function captureOutput(value: unknown): CapturedOutput | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  if (!("type" in value) || !("fileName" in value)) return undefined;
  if (typeof value.fileName !== "string") return undefined;
  if (value.type === "chunk") {
    if (!("code" in value) || typeof value.code !== "string") return undefined;
    const metadata =
      "viteMetadata" in value && typeof value.viteMetadata === "object" && value.viteMetadata !== null ? value.viteMetadata : undefined;
    const importedCss = metadata !== undefined && "importedCss" in metadata ? readStringSet(metadata.importedCss) : undefined;
    const importedAssets = metadata !== undefined && "importedAssets" in metadata ? readStringSet(metadata.importedAssets) : undefined;
    return {
      output: {
        type: "chunk",
        fileName: value.fileName,
        code: value.code,
        facadeModuleId: "facadeModuleId" in value && typeof value.facadeModuleId === "string" ? value.facadeModuleId : undefined,
        moduleIds: "moduleIds" in value ? readStrings(value.moduleIds) : [],
        imports: "imports" in value ? readStrings(value.imports) : [],
        dynamicImports: "dynamicImports" in value ? readStrings(value.dynamicImports) : [],
        isEntry: "isEntry" in value && value.isEntry === true,
        viteMetadata: { importedAssets, importedCss },
      },
    };
  }
  if (value.type === "asset") {
    const output: BundleOutput = {
      type: "asset",
      fileName: value.fileName,
      originalFileName: "originalFileName" in value && typeof value.originalFileName === "string" ? value.originalFileName : undefined,
      originalFileNames: "originalFileNames" in value ? readStrings(value.originalFileNames) : undefined,
    };
    if (value.fileName !== "index.html") return { output };
    if (!("source" in value)) throw new Error("[reze] built template index.html has no readable source");
    return { output, templateHtml: decodeSource(value.source) };
  }
  return undefined;
}
