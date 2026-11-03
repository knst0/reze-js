/** The path of `pattern` with its params filled in: `/posts/:id` with `{ id: 1 }` is `/posts/1`. */
export function buildPaths(pattern: string, params: Record<string, string | number | undefined> = {}): string {
  const parts = pattern.split("/").flatMap((part) => {
    if (part[0] !== ":" && part[0] !== "*") return [part];
    const optional = part.endsWith("?");
    const name = part.slice(1, optional ? -1 : undefined);
    const value = params[name];
    if (value === undefined && !optional && part[0] === ":") {
      throw new Error(`buildPaths: "${pattern}" needs the param "${name}"`);
    }
    if (value === undefined || (value === "" && (optional || part[0] === "*"))) return [];
    return part[0] === "*" ? String(value).split("/").map(encodeURIComponent) : [encodeURIComponent(String(value))];
  });
  return parts.join("/") || "/";
}
