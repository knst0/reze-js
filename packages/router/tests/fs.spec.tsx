import { flush } from "@rezejs/signals";
import { cleanup, mount } from "@rezejs/test-utils";
import { afterEach, beforeEach, expect, test } from "vitest";

import { Outlet, Router, useParams } from "../src";
import { fileRoutes, type FileRouteEntry } from "@rezejs/router/fs";

afterEach(cleanup);
beforeEach(() => window.history.replaceState(null, "", "/"));

async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
  flush();
}

const Home = () => <h1>home</h1>;
const Users = () => (
  <section>
    <h1>users</h1>
    <Outlet />
  </section>
);
const User = () => {
  const params = useParams();
  return <p>user {params.id}</p>;
};

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
          $component: { src: "routes/users/[id].tsx", require: () => ({ default: User }) },
        },
      ],
    },
    { path: "/ghost", $component: undefined },
  ];
}

test("fileRoutes nests pageRoutes and skips entries without a component", async () => {
  window.history.replaceState(null, "", "/users/7");
  const { el } = mount(() => <Router routes={fileRoutes(entries())} />);
  await settle();
  expect(el.innerHTML).toBe("<section><h1>users</h1><p>user 7</p></section>");
});

test("a route preload runs next to the module load", async () => {
  const seen: string[] = [];
  const gate = Promise.withResolvers<Record<string, unknown>>();
  const routes = fileRoutes([
    {
      path: "/",
      page: true,
      $component: { src: "routes/slow.tsx", import: () => gate.promise },
      $$route: {
        require: () => ({
          route: {
            preload: () => {
              seen.push("data");
              return Promise.resolve();
            },
          },
        }),
      },
    },
  ]);
  const preload = (routes[0]?.component as { preload: () => Promise<unknown> } | undefined)
    ?.preload;
  expect(preload).toBeTypeOf("function");
  const pending = preload?.();
  gate.resolve({ default: Home });
  const module = (await pending) as { default: typeof Home };
  expect(seen).toEqual(["data"]);
  expect(module.default).toBe(Home);
});
