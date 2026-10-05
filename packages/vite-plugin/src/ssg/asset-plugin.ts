import { existsSync } from "node:fs";
import { join, relative } from "node:path";

import { attachScopes } from "@rollup/pluginutils";
import type { AttachedScope } from "@rollup/pluginutils";
import MagicString from "magic-string";
import type { Plugin, ResolvedConfig } from "vite";

import { canonicalModuleId } from "../module-identity";
import type { ClientAssetInputs } from "./assets";

interface AssetNode {
  type: string;
  start: number;
  end: number;
  scope?: AttachedScope;
  [key: string]: unknown;
}

function node(value: unknown): value is AssetNode {
  return (
    typeof value === "object" &&
    value !== null &&
    "type" in value &&
    typeof value.type === "string" &&
    "start" in value &&
    typeof value.start === "number" &&
    "end" in value &&
    typeof value.end === "number"
  );
}

function walk(
  value: unknown,
  visit: (current: AssetNode, parent: AssetNode | undefined, scope: AttachedScope | undefined) => void,
  parent?: AssetNode,
  scope?: AttachedScope,
): void {
  if (Array.isArray(value)) {
    for (const item of value) walk(item, visit, parent, scope);
  } else if (node(value)) {
    scope = value.scope ?? scope;
    visit(value, parent, scope);
    for (const key of Object.keys(value)) if (key !== "scope") walk(value[key], visit, value, scope);
  }
}

function stringLiteral(value: unknown): string | undefined {
  if (!node(value)) return undefined;
  if (value.type === "Literal" && typeof value.value === "string") return value.value;
  if (
    value.type === "TemplateLiteral" &&
    Array.isArray(value.expressions) &&
    value.expressions.length === 0 &&
    Array.isArray(value.quasis)
  ) {
    const quasi: unknown = value.quasis[0];
    if (
      node(quasi) &&
      typeof quasi.value === "object" &&
      quasi.value !== null &&
      "cooked" in quasi.value &&
      typeof quasi.value.cooked === "string"
    )
      return quasi.value.cooked;
  }
  return undefined;
}

function isMetaUrl(value: unknown): boolean {
  if (!node(value) || value.type !== "MemberExpression" || value.computed !== false) return false;
  const object = value.object;
  return (
    node(value.property) &&
    value.property.type === "Identifier" &&
    value.property.name === "url" &&
    node(object) &&
    object.type === "MetaProperty" &&
    node(object.meta) &&
    object.meta.name === "import" &&
    node(object.property) &&
    object.property.name === "meta"
  );
}

function cleanId(id: string): string {
  return id.replace(/[?#].*$/, "");
}

function assetPostfix(id: string): string {
  const hash = id.indexOf("#");
  const fragment = hash < 0 ? "" : id.slice(hash);
  const question = id.indexOf("?");
  const query =
    question < 0 || (hash >= 0 && question > hash)
      ? ""
      : id
          .slice(question + 1, hash < 0 ? undefined : hash)
          .split("&")
          .filter((part) => !/^(?:url|inline|no-inline)$/.test(part))
          .join("&");
  return (query ? `?${query}` : "") + fragment;
}

export function createAssetPlugins(htmlEnvironment: string, inputs: ClientAssetInputs): Plugin[] {
  let config: ResolvedConfig;
  const isAsset = (id: string): boolean =>
    !/(?:\?|&)raw(?:&|$)/.test(id) && (config.assetsInclude(cleanId(id)) || /(?:\?|&)url(?:&|$)/.test(id));
  const register = (id: string): string => {
    const file = cleanId(id);
    const publicFile = config.publicDir && file.startsWith("/") ? join(config.publicDir, file.slice(1)) : "";
    const resolved = !existsSync(file) && publicFile && existsSync(publicFile) ? publicFile : file;
    const key = canonicalModuleId(resolved + id.slice(file.length), config.root);
    inputs.files.set(key, { id: canonicalModuleId(resolved, config.root), postfix: assetPostfix(id) });
    if (resolved === publicFile)
      inputs.publicFiles.set(key, encodeURI(relative(config.publicDir, resolved).replace(/\\/g, "/")) + assetPostfix(id));
    return key;
  };
  const loader: Plugin = {
    name: "reze-ssg-asset-modules",
    enforce: "pre",
    apply: "build",
    configResolved(resolved) {
      config = resolved;
    },
    load(id) {
      if (this.environment.name !== htmlEnvironment || !isAsset(id)) return;
      const key = register(id);
      return `import { htmlAsset } from "reze-js/internal/html"; export default htmlAsset(${JSON.stringify(key)});`;
    },
  };
  const syntax: Plugin = {
    name: "reze-ssg-asset-syntax",
    apply: "build",
    async transform(code, id) {
      const isHtml = this.environment.name === htmlEnvironment;
      if (!isHtml && this.environment.name !== "client") return;
      if (isAsset(id)) {
        if (isHtml) return;
        const ast = this.parse(code);
        walk(ast, (current) => {
          if (current.type !== "ExportDefaultDeclaration") return;
          const value = stringLiteral(current.declaration);
          if (value?.startsWith("data:")) inputs.inlined.set(register(id), value);
        });
        return;
      }
      if (!code.includes("import.meta.url")) return;
      const ast = this.parse(code);
      const rootScope = attachScopes(ast);
      const names = new Set<string>();
      const urls: { expression: AssetNode; replace: AssetNode; specifier: string }[] = [];
      walk(
        ast,
        (current, parent, scope) => {
          if (current.type === "Identifier" && typeof current.name === "string") names.add(current.name);
          if (
            current.type !== "NewExpression" ||
            !node(current.callee) ||
            current.callee.type !== "Identifier" ||
            current.callee.name !== "URL" ||
            scope?.contains("URL") ||
            !Array.isArray(current.arguments) ||
            current.arguments.length !== 2 ||
            !isMetaUrl(current.arguments[1])
          )
            return;
          const specifier = stringLiteral(current.arguments[0]);
          if (specifier === undefined || (!specifier.startsWith("./") && !specifier.startsWith("../"))) return;
          const href =
            parent?.type === "MemberExpression" &&
            parent.object === current &&
            parent.computed === false &&
            node(parent.property) &&
            parent.property.type === "Identifier" &&
            parent.property.name === "href";
          urls.push({ expression: current, replace: isHtml && href ? parent : current, specifier });
        },
        undefined,
        rootScope,
      );
      if (urls.length === 0) return;
      const output = new MagicString(code);
      const imports: string[] = [];
      let ordinal = 0;
      for (const url of urls) {
        const specifier = decodeURI(url.specifier);
        const resolved = await this.resolve(specifier, id);
        if (resolved === null || !isAsset(resolved.id)) continue;
        let local: string;
        do {
          local = `__reze_asset_${ordinal++}`;
        } while (names.has(local));
        names.add(local);
        imports.push(`import ${local} from ${JSON.stringify(specifier)};`);
        if (isHtml) output.overwrite(url.replace.start, url.replace.end, local);
        else {
          const args = url.expression.arguments;
          if (Array.isArray(args) && node(args[0])) output.overwrite(args[0].start, args[0].end, local);
        }
      }
      if (imports.length === 0) return;
      output.prepend(imports.join("\n") + "\n");
      return { code: output.toString(), map: output.generateMap({ hires: true, source: id, includeContent: true }) };
    },
  };
  return [loader, syntax];
}
