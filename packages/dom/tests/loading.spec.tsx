import { flush, signal } from "@rezejs/signals";
import { cleanup, mount } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

import { Loading, startTransition, useTransition } from "../src/loading";

afterEach(cleanup);

function tick(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

test("Loading shows fallback until the async component settles", async () => {
  const gate = Promise.withResolvers<string>();
  async function Card() {
    const name = await gate.promise;
    return <b>{name}</b>;
  }
  const { el } = mount(() => (
    <Loading fallback={<i>loading</i>}>
      <Card />
    </Loading>
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
    <Loading fallback={<i>loading</i>}>
      <Card id={id()} />
    </Loading>
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
    <Loading fallback={<i>loading</i>}>
      <Card />
    </Loading>
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
    <Loading fallback={<i>loading</i>}>
      <Card />
    </Loading>
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

function deferredCard() {
  const gates = new Map<number, PromiseWithResolvers<string>>();
  const gate = (id: number) => {
    if (!gates.has(id)) gates.set(id, Promise.withResolvers<string>());
    return gates.get(id)!;
  };
  async function Card(props: { id: number }) {
    const name = await gate(props.id).promise;
    return <b>{name}</b>;
  }
  return { Card, gate };
}

test("no wrapper element: the children sit directly in the parent", async () => {
  const { Card, gate } = deferredCard();
  const { el } = mount(
    () => (
      <Loading fallback={<tr>loading</tr>}>
        <Card id={1} />
      </Loading>
    ),
    "tbody",
  );
  gate(1).resolve("ada");
  await tick();
  flush();
  expect(el.innerHTML).toBe("<b>ada</b>");
});

test("an async component created later, in a branch, still reaches the boundary", async () => {
  const { Card, gate } = deferredCard();
  const [open, setOpen] = signal(false);
  const { el } = mount(() => (
    <Loading fallback={<i>loading</i>}>
      <p>{open() ? <Card id={1} /> : "closed"}</p>
    </Loading>
  ));
  expect(el.innerHTML).toBe("<p>closed</p>");
  setOpen(true);
  flush();
  expect(el.innerHTML).toBe("<i>loading</i>");
  gate(1).resolve("ada");
  await tick();
  flush();
  expect(el.innerHTML).toBe("<p><b>ada</b></p>");
});

test("a transition keeps shown content until the new work settles", async () => {
  const { Card, gate } = deferredCard();
  const [id, setId] = signal(1);
  const [isPending, start] = useTransition();
  const { el } = mount(() => (
    <Loading fallback={<i>loading</i>}>
      <Card id={id()} />
    </Loading>
  ));
  gate(1).resolve("ada");
  await tick();
  flush();
  expect(el.innerHTML).toBe("<b>ada</b>");

  let settled = false;
  void start(() => setId(2)).then(() => (settled = true));
  flush();
  expect(isPending()).toBe(true);
  expect(el.innerHTML).toBe("<b>ada</b>");
  gate(2).resolve("grace");
  await tick();
  flush();
  expect(el.innerHTML).toBe("<b>grace</b>");
  expect(isPending()).toBe(false);
  expect(settled).toBe(true);
});

test("without a transition the fallback shows again; a first reveal inside a transition still falls back", async () => {
  const { Card, gate } = deferredCard();
  const [id, setId] = signal(1);
  const { el } = mount(() => (
    <Loading fallback={<i>loading</i>}>
      <Card id={id()} />
    </Loading>
  ));
  void startTransition(() => {});
  expect(el.innerHTML).toBe("<i>loading</i>");
  gate(1).resolve("ada");
  await tick();
  flush();
  setId(2);
  flush();
  expect(el.innerHTML).toBe("<i>loading</i>");
});

test("`on` shows the fallback only when it changed", async () => {
  const { Card, gate } = deferredCard();
  const [route, setRoute] = signal("a");
  const [id, setId] = signal(1);
  const { el } = mount(() => (
    <Loading fallback={<i>loading</i>} on={route()}>
      <Card id={id()} />
    </Loading>
  ));
  gate(1).resolve("one");
  await tick();
  flush();
  setId(2);
  flush();
  expect(el.innerHTML).toBe("<b>one</b>");
  gate(2).resolve("two");
  await tick();
  flush();
  expect(el.innerHTML).toBe("<b>two</b>");
  setRoute("b");
  setId(3);
  flush();
  expect(el.innerHTML).toBe("<i>loading</i>");
});
