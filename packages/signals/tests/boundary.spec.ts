import { expect, test } from "vitest";

import { asyncComputed, boundary, effect, flush, signal } from "../src";

function tick(): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, 0);
  return promise;
}

function request<T>() {
  const deferred = Promise.withResolvers<T>();
  const data = asyncComputed(() => deferred.promise);
  return { data, ...deferred };
}

test("a read of an unsettled async computation inside a boundary is pending until it settles", async () => {
  const { data, resolve } = request<string>();
  const [, scope] = boundary(() => effect(() => void data.value()));
  expect(scope.isPending()).toBe(true);
  resolve("a");
  await tick();
  flush();
  expect(scope.isPending()).toBe(false);
});
test("a rejected first run also clears pending", async () => {
  const { data, reject } = request<string>();
  const [, scope] = boundary(() => effect(() => void data.value()));
  expect(scope.isPending()).toBe(true);
  reject(new Error("no"));
  await tick();
  flush();
  expect(scope.isPending()).toBe(false);
});

test("disposing the reader before settlement clears pending at once", () => {
  const { data } = request<string>();
  let stop!: () => void;
  const [, scope] = boundary(() => {
    stop = effect(() => void data.value());
  });
  expect(scope.isPending()).toBe(true);
  stop();
  expect(scope.isPending()).toBe(false);
});

test("only readers inside the nearest boundary count", () => {
  const { data } = request<string>();
  const outside = effect(() => void data.value());
  const [inner, outer] = boundary(() => boundary(() => effect(() => void data.value())));
  expect(inner[1].isPending()).toBe(true);
  expect(outer.isPending()).toBe(false);
  const [, empty] = boundary(() => undefined);
  expect(empty.isPending()).toBe(false);
  outside();
});

test("a reload after the first settle keeps the boundary settled", async () => {
  const [id, setId] = signal(1);
  const data = asyncComputed((c) => Promise.resolve(c.get(id)));
  const [, scope] = boundary(() => effect(() => void data.value()));
  expect(scope.isPending()).toBe(true);
  await tick();
  flush();
  expect(scope.isPending()).toBe(false);
  setId(2);
  flush();
  expect(scope.isPending()).toBe(false);
  await tick();
  flush();
  expect(scope.isPending()).toBe(false);
});
