import { expect, test } from "vitest";

import { trackAsync } from "../src/async";
import { getOwner, onCleanup, root, type Owner } from "../src";

function tick(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

test("fresh resolution applies its setter back under the captured owner", async () => {
  const log: string[] = [];
  let owner: Owner | undefined;
  const dispose = root((dispose) => {
    owner = getOwner();
    return dispose;
  });
  let seen = -1;
  trackAsync(
    owner,
    Promise.resolve(42),
    () => true,
    (v) => {
      seen = v;
      onCleanup(() => log.push("adopted"));
    },
  );
  await tick();
  expect(seen).toBe(42);
  dispose();
  expect(log).toEqual(["adopted"]);
});

test("stale generation never touches state", async () => {
  let seen = -1;
  let gen = 0;
  const my = gen;
  trackAsync(
    undefined,
    Promise.resolve(7),
    () => my === gen,
    (v) => {
      seen = v;
    },
  );
  gen++;
  await tick();
  expect(seen).toBe(-1);
});

test("rejection routes to setError under the owner, stale rejections drop", async () => {
  let error: unknown;
  let owner: Owner | undefined;
  const dispose = root((dispose) => {
    owner = getOwner();
    return dispose;
  });
  trackAsync(
    owner,
    Promise.reject(new Error("boom")),
    () => true,
    () => {},
    (e) => {
      error = e;
      onCleanup(() => {});
    },
  );
  let stale = 0;
  trackAsync(
    owner,
    Promise.reject(new Error("stale")),
    () => false,
    () => {},
    () => {
      stale++;
    },
  );
  await tick();
  expect((error as Error).message).toBe("boom");
  expect(stale).toBe(0);
  dispose();
});

test("component-level cleanup invalidates pending promises (generated pattern)", async () => {
  const log: string[] = [];
  let owner: Owner | undefined;
  let gen = 0;
  const dispose = root((dispose) => {
    owner = getOwner();
    onCleanup(() => {
      gen++;
    });
    return dispose;
  });
  const { promise, resolve } = Promise.withResolvers<string>();
  const my = gen;
  trackAsync(owner, promise, () => my === gen, () => {
    log.push("late setter");
  });
  dispose();
  resolve("too late");
  await tick();
  expect(log).toEqual([]);
  expect(gen).toBe(1);
});
