/// <reference types="../client.d.ts" />
import { join } from "node:path";

import { flushSync } from "@rezejs/signals";
import { cleanup, mount } from "@rezejs/test-utils";
import routes from "virtual:reze-routes";
import { afterEach, expect, test } from "vitest";

import { preloadRoutes, Router } from "../src";
import { routesModule, scanRoutes } from "../src/vite";

const routesDir = join(import.meta.dirname, "fixtures/routes");

afterEach(cleanup);

test("files map to a route tree", () => {
  expect(scanRoutes(routesDir)).toEqual([
    { path: "pricing", file: "(marketing)/pricing.tsx" },
    { path: "docs/:page?", file: "docs/[[page]].tsx" },
    {
      path: "users",
      file: "users.tsx",
      children: [
        { path: ":id", file: "users/[id].tsx" },
        { path: "/", file: "users/index.tsx" },
      ],
    },
    { path: "*rest", file: "[...rest].tsx" },
    { path: "about", file: "about.tsx" },
    { path: "/", file: "index.tsx" },
  ]);
});

test("the module imports each route lazily", () => {
  const code = routesModule([{ path: "/", file: "index.tsx" }], "/app/src/routes");
  expect(code).toBe(
    'export default [\n  { path: "/", load: () => import("/app/src/routes/index.tsx") },\n];\n',
  );
});

async function render(url: string): Promise<string> {
  window.history.replaceState(null, "", url);
  await preloadRoutes(routes, url);
  const { el } = mount(() => <Router routes={routes} />);
  flushSync();
  const html = el.innerHTML;
  cleanup();
  return html;
}

test("virtual:reze-routes renders through the router", async () => {
  expect(await render("/")).toBe("<h1>home</h1>");
  expect(await render("/about")).toBe("<h1>about</h1>");
  expect(await render("/users")).toBe("<section><h1>users</h1><p>all users</p></section>");
  expect(await render("/users/7")).toBe("<section><h1>users</h1><p>user 7</p></section>");
  expect(await render("/pricing")).toBe("<h1>pricing</h1>");
  expect(await render("/docs")).toBe("<p>docs index</p>");
  expect(await render("/docs/intro")).toBe("<p>docs intro</p>");
  expect(await render("/a/b")).toBe("<p>missing a/b</p>");
});

test("root wraps every route", async () => {
  window.history.replaceState(null, "", "/about");
  await preloadRoutes(routes, "/about");
  const { el } = mount(() => (
    <Router routes={routes} root={(props) => <main>{props.children}</main>} />
  ));
  expect(el.innerHTML).toBe("<main><h1>about</h1></main>");
});
