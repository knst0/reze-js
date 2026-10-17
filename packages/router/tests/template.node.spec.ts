import { expect, test } from "vite-plus/test";

import { scanRoutes } from "../src/fs/scan";
import { routeTemplate } from "../src/fs/template";

function template(files: readonly string[], file: string, isLayout = false): string | undefined {
  const queue = scanRoutes(files);
  for (let route = queue.pop(); route !== undefined; route = queue.pop()) {
    if (route.file === file) return routeTemplate(route, isLayout);
    queue.push(...route.children);
  }
  throw new Error(`missing route ${file}`);
}

test("routeTemplate types a page by its full pattern and names it after the file", () => {
  expect(template(["blog.tsx", "blog/[id].tsx"], "blog/[id].tsx")).toBe(
    'import type { RoutePropsFor } from "@rezejs/router";\n\nexport default function BlogIdPage(props: RoutePropsFor<"/blog/:id">) {\n  return <h1>{props.location.pathname}</h1>;\n}\n',
  );
  expect(template(["(shell)/[...404].jsx"], "(shell)/[...404].jsx")).toBe(
    "export default function Shell404Page(props) {\n  return <h1>{props.location.pathname}</h1>;\n}\n",
  );
  expect(template(["[...404].jsx"], "[...404].jsx")).toContain("function Route404Page(props)");
});

test("routeTemplate renders children for a layout and skips files without JSX", () => {
  expect(template(["(shell).tsx"], "(shell).tsx", true)).toBe(
    'import type { RouteProps } from "@rezejs/router";\n\nexport default function ShellLayout(props: RouteProps) {\n  return <>{props.children}</>;\n}\n',
  );
  expect(template(["docs.jsx"], "docs.jsx", true)).toBe(
    "export default function DocsLayout(props) {\n  return <>{props.children}</>;\n}\n",
  );
  expect(template(["api.ts"], "api.ts")).toBeUndefined();
});
