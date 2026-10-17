import type { FileRoute } from "./scan";

function componentName(id: string, suffix: string): string {
  const words = id.split(/[^A-Za-z0-9]+/).filter((word) => word !== "");
  const name = words.map((word) => word.slice(0, 1).toUpperCase() + word.slice(1)).join("");
  return (/^[A-Za-z]/.test(name) ? name : "Route" + name) + suffix;
}

/** Starter source for a new route file: a layout rendering `children`, else a page typed by its pattern; `undefined` outside `.tsx` and `.jsx`. */
export function routeTemplate(route: FileRoute, isLayout: boolean): string | undefined {
  const isTyped = route.file.endsWith(".tsx");
  if (!isTyped && !route.file.endsWith(".jsx")) return undefined;
  if (isLayout) {
    const name = componentName(route.id, "Layout");
    if (!isTyped) return `export default function ${name}(props) {\n  return <>{props.children}</>;\n}\n`;
    return `import type { RouteProps } from "@rezejs/router";\n\nexport default function ${name}(props: RouteProps) {\n  return <>{props.children}</>;\n}\n`;
  }
  const name = componentName(route.id, "Page");
  const body = `  return <h1>{props.location.pathname}</h1>;\n}\n`;
  if (!isTyped) return `export default function ${name}(props) {\n${body}`;
  return `import type { RoutePropsFor } from "@rezejs/router";\n\nexport default function ${name}(props: RoutePropsFor<${JSON.stringify(route.fullPath)}>) {\n${body}`;
}
