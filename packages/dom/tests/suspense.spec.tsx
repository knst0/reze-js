import { flush, signal } from "@rezejs/signals";
import { cleanup, mount } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

import { Suspense } from "../src/flow";

afterEach(cleanup);

function tick(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

test("Suspense shows fallback until the async component settles", async () => {
  const gate = Promise.withResolvers<string>();
  async function Card() {
    const name = await gate.promise;
    return <b>{name}</b>;
  }
  const { el } = mount(() => (
    <Suspense fallback={<i>loading</i>}>
      <Card />
    </Suspense>
  ));
  expect(el.innerHTML).toContain("loading");
  gate.resolve("ada");
  await tick();
  flush();
  expect(el.innerHTML).toContain("<b>ada</b>");
  expect(el.innerHTML).not.toContain("loading");
});

test("a stale resolution never paints", async () => {
  const slow = Promise.withResolvers<string>();
  const fast = Promise.withResolvers<string>();
  function fetchName(id: number): Promise<string> {
    return id === 1 ? slow.promise : fast.promise;
  }
  async function Card(props: { id: number }) {
    const name = await fetchName(props.id);
    return <b>{name}</b>;
  }
  const [id, setId] = signal(1);
  const { el } = mount(() => (
    <Suspense fallback={<i>loading</i>}>
      <Card id={id()} />
    </Suspense>
  ));
  setId(2);
  flush();
  fast.resolve("grace");
  await tick();
  flush();
  expect(el.innerHTML).toContain("grace");
  slow.resolve("ada");
  await tick();
  flush();
  expect(el.innerHTML).toContain("grace");
  expect(el.innerHTML).not.toContain("ada");
});

test("unmounting while pending drops the late resolve", async () => {
  const gate = Promise.withResolvers<string>();
  async function Card() {
    const name = await gate.promise;
    return <b>{name}</b>;
  }
  const { el, dispose } = mount(() => (
    <Suspense fallback={<i>loading</i>}>
      <Card />
    </Suspense>
  ));
  expect(el.innerHTML).toContain("loading");
  dispose();
  gate.resolve("ada");
  await tick();
  flush();
  expect(el.innerHTML).not.toContain("ada");
});

test("chained awaits resolve in order through one boundary", async () => {
  const first = Promise.withResolvers<string>();
  const second = Promise.withResolvers<string>();
  async function Card() {
    const a = await first.promise;
    const b = await second.promise.then((tail) => `${a}/${tail}`);
    return <b>{b}</b>;
  }
  const { el } = mount(() => (
    <Suspense fallback={<i>loading</i>}>
      <Card />
    </Suspense>
  ));
  expect(el.innerHTML).toContain("loading");
  first.resolve("x");
  await tick();
  flush();
  expect(el.innerHTML).toContain("loading");
  second.resolve("y");
  await tick();
  flush();
  expect(el.innerHTML).toContain("<b>x/y</b>");
  expect(el.innerHTML).not.toContain("loading");
});
