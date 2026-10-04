export type TrailingSlash = "always" | "never";

const ReservedNames: Record<string, true> = {
  con: true, prn: true, aux: true, nul: true,
  com1: true, com2: true, com3: true, com4: true, com5: true, com6: true, com7: true, com8: true, com9: true,
  lpt1: true, lpt2: true, lpt3: true, lpt4: true, lpt5: true, lpt6: true, lpt7: true, lpt8: true, lpt9: true,
};

export function normalizePageUrl(raw: string): string {
  const withoutSuffix = raw.split(/[?#]/, 1)[0] ?? "";
  if (!withoutSuffix.startsWith("/")) {
    throw new Error(`[reze] SSG page URL must be an absolute path, got ${JSON.stringify(raw)}`);
  }
  if (withoutSuffix.includes("\0")) {
    throw new Error(`[reze] SSG page URL must not contain NUL: ${JSON.stringify(raw)}`);
  }
  const kept: string[] = [];
  for (const part of withoutSuffix.split("/").slice(1)) {
    if (part === "" || part === ".") continue;
    if (part === "..") throw new Error(`[reze] SSG page URL must not traverse: ${JSON.stringify(raw)}`);
    let decoded = part;
    try {
      decoded = decodeURIComponent(part);
    } catch {
      throw new Error(`[reze] SSG page URL has malformed encoding: ${JSON.stringify(raw)}`);
    }
    if (decoded === "" || decoded === "." || decoded === ".." || /[/\\]/.test(decoded)) {
      throw new Error(`[reze] SSG page URL must not traverse or embed an encoded slash: ${JSON.stringify(raw)}`);
    }
    if (/[\0-\x1f\x7f]/.test(decoded) || decoded.length > 255 || decoded.endsWith(".") || decoded.endsWith(" ")) {
      throw new Error(`[reze] SSG page URL has an unsafe segment: ${JSON.stringify(raw)}`);
    }
    if (ReservedNames[decoded.toLowerCase().split(".")[0]!] === true) {
      throw new Error(`[reze] SSG page URL has a platform-reserved segment: ${JSON.stringify(raw)}`);
    }
    kept.push(decoded);
  }
  return `/${kept.join("/")}`;
}

export function canonicalPageUrl(pathname: string, trailingSlash: TrailingSlash): string {
  if (pathname === "/") return "/";
  return trailingSlash === "always" ? `${pathname}/` : pathname;
}

export function outputFileFor(pathname: string): string {
  if (pathname === "/") return "index.html";
  return `${pathname.slice(1, pathname.endsWith("/") ? -1 : undefined)}/index.html`;
}

export function joinBase(base: string, file: string, depth: number): string {
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(base) || base.startsWith("//")) {
    return `${base.replace(/\/+$/, "")}/${file}`;
  }
  if (base.startsWith("/")) {
    return `${base.replace(/\/+$/, "")}/${file}`;
  }
  const prefix = depth === 0 ? "./" : "../".repeat(depth);
  return `${prefix}${file}`;
}

export function pageDepth(pathname: string): number {
  let depth = 0;
  for (let index = 1; index < pathname.length; index++) {
    if (pathname.charCodeAt(index) === 47) depth++;
  }
  return depth;
}

export function planOutputs(urls: readonly string[]): Map<string, string> {
  const byFile = new Map<string, string>();
  const byFolded = new Map<string, string>();
  for (const url of urls) {
    const file = outputFileFor(url);
    const clash = byFile.get(file);
    if (clash !== undefined) {
      throw new Error(`[reze] duplicate SSG output ${JSON.stringify(file)} for ${JSON.stringify(clash)} and ${JSON.stringify(url)}`);
    }
    byFile.set(file, url);
    const folded = file.toLowerCase();
    const foldedClash = byFolded.get(folded);
    if (foldedClash !== undefined && foldedClash !== file) {
      throw new Error(`[reze] SSG outputs collide after normalization: ${JSON.stringify(foldedClash)} and ${JSON.stringify(file)}`);
    }
    byFolded.set(folded, file);
  }
  return byFile;
}
