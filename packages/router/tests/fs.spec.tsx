import { flush } from "@rezejs/signals";
import { cleanup, mount } from "@rezejs/test-utils";
import { afterEach, beforeEach, expect, test } from "vitest";

import { createRouter, defineRoutes, memoryHistory } from "../src";
import { defineFileRoute, fileRoutes, type FileRouteEntry } from "../src/fs";

afterEach(cleanup);
beforeEach(() => window.history.replaceState(null, "", "/"));

const Home = () => <h1>home</h1>;
const Users = (props: { children?: unknown }) => (
  <>
    <h1>users</h1>
    {props.children as never}
  </>
);

function entries(): FileRouteEntry[] {
  return [
    {
      path: "/",
      page: true,
      $component: { src: "routes/index.tsx", import: () => Promise.resolve({ default: Home }) },
    },
    {
      path: "/users",
      page: true,
      $component: { src: "routes/users.tsx", import: () => Promise.resolve({ default: Users }) },
      children: [
        {
          path: "/:id",
          page: true,
          $component: {
            src: "routes/users.tsx",
            import: () => Promise.resolve({ default: Users }),
          },
        },
        {
          path: "/new",
          page: true,
          $component: { require: () => ({ default: Home }) },
        },
      ],
    },
    { path: "/ghost" },
  ];
}

test("fileRoutes maps every entry with component, config and children", () => {
  const routes = fileRoutes(entries());
  expect(routes).toHaveLength(3);
  expect(routes[0]).toMatchObject({ path: "/", info: { filesystem: true } });
  expect(routes[2]).toMatchObject({ path: "/ghost", component: undefined });
  const users = routes[1]!;
  expect(users.path).toBe("/users");
  expect(users.children).toHaveLength(2);
});

test("eager refs pass through, lazy refs share by source", () => {
  const routes = fileRoutes(entries());
  const users = routes[1]!;
  const [param, fresh] = users.children as unknown as { component: unknown }[];
  expect(param!.component).toBe(users.component);
  expect(fresh!.component).toBe(Home);
});

test("a route export spreads into the definition", () => {
  const routes = fileRoutes([
    {
      path: "/post",
      page: true,
      $component: { src: "routes/post.tsx", import: () => Promise.resolve({ default: Home }) },
      $$route: {
        require: () => ({ route: defineFileRoute("/post", { info: { breadcrumb: "Post" } }) }),
      },
    },
  ]);
  expect(routes[0]).toMatchObject({
    path: "/post",
    info: { breadcrumb: "Post", filesystem: true },
  });
});

test("file routes render through the router", async () => {
  const history = memoryHistory("/users/new");
  const App = createRouter({ history, routes: fileRoutes(entries()) });
  const { el } = mount(() => <App />);
  for (let round = 0; round < 3; round++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flush();
  }
  expect(el.innerHTML).toBe("<h1>users</h1><h1>home</h1>");
});

test("defineFileRoute keeps its config", () => {
  const route = defineFileRoute("/blog/:id", {
    preload: ({ params }) => `post-${params.id}`,
    info: { breadcrumb: "Blog" },
  });
  expect(
    route.preload!({ params: { id: "7" }, location: undefined as never, intent: "initial" }),
  ).toBe("post-7");
  expect(route.info).toEqual({ breadcrumb: "Blog" });
});

test("manifest tuples keep literal paths", () => {
  const routes = fileRoutes([
    {
      path: "/",
      $component: { src: "a", import: () => Promise.resolve({ default: Home }) },
    },
    {
      path: "/users/:id",
      $component: { src: "b", import: () => Promise.resolve({ default: Users }) },
    },
  ] as const);
  expect(defineRoutes(routes).length).toBe(2);
});
