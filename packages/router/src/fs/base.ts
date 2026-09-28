const UrlOrRelative = /^(?:\.|\/\/|[a-z][a-z\d+.-]*:)/i;

/** `base` as a path prefix without a trailing slash; `""` for the root and for relative or URL bases, which name no path. */
export function routerBase(base: string): string {
  return UrlOrRelative.test(base) ? "" : ("/" + base).replace(/\/+/g, "/").replace(/\/$/, "");
}
