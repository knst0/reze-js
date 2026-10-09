import type { SourceSite } from "@rezejs/signals/internal/scope";

export class RenderError extends Error {
  constructor(message: string, site?: SourceSite) {
    super(site === undefined ? `reze: ${message}` : `reze: ${message} at ${site.module}:${site.line}:${site.column} (site ${site.key})`);
    this.name = "RenderError";
  }
}
