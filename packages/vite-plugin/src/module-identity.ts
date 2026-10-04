import { createHash } from "node:crypto";
import { posix } from "node:path";

export interface CanonicalIdOptions {
  packageName?: string;
  packageRoot?: string;
}

function normalizePath(value: string): string {
  return posix.normalize(value.replace(/\\/g, "/"));
}

function relativeWithin(root: string, file: string): string | undefined {
  const relative = posix.relative(root, file);
  return relative === ".." || relative.startsWith("../") || posix.isAbsolute(relative) ? undefined : relative;
}

function splitId(rawId: string): { path: string; suffix: string } {
  const hash = rawId.indexOf("#");
  const fragment = hash < 0 ? "" : rawId.slice(hash);
  const beforeHash = hash < 0 ? rawId : rawId.slice(0, hash);
  const question = beforeHash.indexOf("?");
  if (question < 0) return { path: beforeHash, suffix: fragment };
  const query = beforeHash.slice(question + 1).split("&").filter((part) => !/^t=\d{13}$/.test(part)).join("&");
  return { path: beforeHash.slice(0, question), suffix: (query === "" ? "" : `?${query}`) + fragment };
}

/** External linked files require the owning package name and directory when no node_modules boundary exists. */
export function canonicalModuleId(rawId: string, root: string, options?: CanonicalIdOptions): string {
  const { path, suffix } = splitId(rawId);
  if (path.startsWith("\0")) {
    const name = path.slice(1);
    return (name.startsWith("virtual:") ? name : `virtual:${name}`) + suffix;
  }
  if (path.startsWith("virtual:")) return path + suffix;
  const normalized = normalizePath(path);
  const rootPath = normalizePath(root);
  const file = normalized.startsWith("/") || /^[A-Za-z]:\//.test(normalized)
    ? normalized
    : posix.join(rootPath, normalized);
  const relative = relativeWithin(rootPath, file);
  if (relative !== undefined) return (relative || ".") + suffix;
  if (options?.packageName && options.packageRoot) {
    const packageRelative = relativeWithin(normalizePath(options.packageRoot), file);
    if (packageRelative !== undefined) return options.packageName + (packageRelative ? `/${packageRelative}` : "") + suffix;
  }
  const boundary = "/node_modules/";
  const vendorIndex = file.lastIndexOf(boundary);
  if (vendorIndex >= 0) return file.slice(vendorIndex + boundary.length) + suffix;
  throw new Error(`[reze] Cannot identify the owning package of external module ${JSON.stringify(rawId)}`);
}

/** Hashes canonical module ID, NUL, and the exact pre-define compiler input, in UTF-8. */
export function compilerInputHash(canonicalId: string, input: string): string {
  return createHash("sha256").update(canonicalId).update("\0").update(input).digest("hex").slice(0, 16);
}

export interface ModuleRegistry {
  /** Rejects differing inputs for one module ID and digest collisions across module IDs. */
  register(canonicalId: string, input: string): string;
}

export function createModuleRegistry(): ModuleRegistry {
  const inputs = new Map<string, { input: string; hash: string }>();
  const owners = new Map<string, string>();
  return {
    register(canonicalId, input) {
      const known = inputs.get(canonicalId);
      if (known !== undefined) {
        if (known.input !== input) {
          throw new Error(`[reze] Module ${JSON.stringify(canonicalId)} has different compiler input across targets`);
        }
        return known.hash;
      }
      const hash = compilerInputHash(canonicalId, input);
      const owner = owners.get(hash);
      if (owner !== undefined && owner !== canonicalId) {
        throw new Error(`[reze] Compiler site hash collision ${hash}: ${JSON.stringify(owner)} and ${JSON.stringify(canonicalId)}`);
      }
      owners.set(hash, canonicalId);
      inputs.set(canonicalId, { input, hash });
      return hash;
    },
  };
}
