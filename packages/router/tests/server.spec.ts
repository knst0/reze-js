import { expect, test } from "vitest";

import { createRouter, defineRoute, defineRoutes, query } from "../src";
import { createFlightDataCollector } from "../src/server";

function outcome(targetUrl: string, revalidateKeys?: string[] | true) {
  return {
    request: new Request("http://app/mutate", {
      headers: { referer: "http://app/" },
    }),
    targetUrl,
    revalidateKeys,
  };
}

test("collects preload query values for the target url", async () => {
  const getUser = query(async (id: string) => `user-${id}`, "users");
  const collect = createFlightDataCollector({
    routes: defineRoutes([
      defineRoute({
        path: "/users/:id",
        preload: ({ params }) => getUser(params.id!),
        component: () => null,
      }),
    ]),
  });
  const payload = await collect(undefined, outcome("http://app/users/7", true));
  expect(payload).toEqual({ [getUser.keyFor("7")]: "user-7" });
});

test("runs the root preload with initial intent", async () => {
  const seen: string[] = [];
  const collect = createFlightDataCollector({
    rootPreload: ({ intent }) => void seen.push(intent),
    routes: [{ path: "/", component: () => null }],
  });
  await collect(undefined, outcome("http://app/", true));
  expect(seen).toEqual(["initial"]);
});

test("revalidation keys scope collection to matching entries", async () => {
  const getUser = query(async (id: string) => `user-${id}`, "scoped-users");
  const getPost = query(async (id: string) => `post-${id}`, "scoped-posts");
  const routes = defineRoutes([
    defineRoute({
      path: "/both/:id",
      preload: ({ params }) => [getUser(params.id!), getPost(params.id!)],
      component: () => null,
    }),
  ]);
  const scoped = createFlightDataCollector({ routes });
  const filtered = await collect_scoped(scoped);
  expect(Object.keys(filtered!)).toEqual([getUser.keyFor("1")]);
  const full = await scoped(undefined, outcome("http://app/both/1"));
  expect(Object.keys(full!).sort()).toEqual([getUser.keyFor("1"), getPost.keyFor("1")].sort());

  async function collect_scoped(
    collect: ReturnType<typeof createFlightDataCollector>,
  ): Promise<Record<string, unknown> | undefined> {
    return collect(undefined, outcome("http://app/both/1", ["scoped-users"]));
  }
});

test("accepts a router instance directly", async () => {
  const getValue = query(async () => "v", "instance-value");
  const Router = createRouter({
    routes: [{ path: "/thing", preload: () => getValue(), component: () => null }],
  });
  const collect = createFlightDataCollector(Router);
  const payload = await collect(undefined, outcome("http://app/thing", true));
  expect(payload).toEqual({ [getValue.keyFor()]: "v" });
});

test("returns undefined without a target url or collected values", async () => {
  const collect = createFlightDataCollector({ routes: [{ path: "/" }] });
  await expect(
    collect(undefined, { request: new Request("http://app/mutate") }),
  ).resolves.toBeUndefined();
  await expect(collect(undefined, outcome("http://app/unmatched", true))).resolves.toBeUndefined();
});

test("lazy subtrees resolve before the preload pass", async () => {
  const getDeep = query(async () => "deep", "deep");
  const collect = createFlightDataCollector({
    routes: [
      {
        path: "/feature",
        children: () =>
          Promise.resolve([{ path: "deep", preload: () => getDeep(), component: () => null }]),
      },
    ],
  });
  const payload = await collect(undefined, outcome("http://app/feature/deep", true));
  expect(payload).toEqual({ [getDeep.keyFor()]: "deep" });
});
