import { flush, signal } from "@rezejs/signals";
import { cleanup, mount } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

import { Match, Show, Switch } from "../src/flow";
import { For, Repeat } from "../src/list";
import { Loading, Reveal } from "../src/loading";

afterEach(cleanup);

function tick(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

test("keyed Show hands the raw value and rebuilds when its identity changes", () => {
  let builds = 0;
  const [user, setUser] = signal<{ name: string } | null>({ name: "a" });
  const { el } = mount(() => (
    <Show when={user()} keyed fallback={<i>none</i>}>
      {(u: { name: string }) => {
        builds++;
        return <b>{u.name}</b>;
      }}
    </Show>
  ));
  setUser({ name: "b" });
  flush();
  expect(el.innerHTML).toBe("<b>b</b>");
  expect(builds).toBe(2);
  setUser(null);
  flush();
  expect(el.innerHTML).toBe("<i>none</i>");
});

test("keyed Match rebuilds on identity; plain Match keeps its branch", () => {
  const [value, setValue] = signal({ n: 1 });
  let keyedBuilds = 0;
  let plainBuilds = 0;
  const { el } = mount(() => (
    <>
      <Switch>
        <Match when={value()} keyed>
          {(v: { n: number }) => {
            keyedBuilds++;
            return <b>{v.n}</b>;
          }}
        </Match>
      </Switch>
      <Switch>
        <Match when={value()}>
          {(v: () => { n: number }) => {
            plainBuilds++;
            return <i>{v().n}</i>;
          }}
        </Match>
      </Switch>
    </>
  ));
  setValue({ n: 2 });
  flush();
  expect(el.innerHTML).toBe("<b>2</b><i>2</i>");
  expect([keyedBuilds, plainBuilds]).toEqual([2, 1]);
});

test("For keyed={false} keeps rows by position and updates their item", () => {
  const [items, setItems] = signal(["a", "b", "c"]);
  let created = 0;
  const { el } = mount(
    () => (
      <For each={items()} keyed={false}>
        {(item, index) => {
          created++;
          return (
            <li>
              {index}:{item()}
            </li>
          );
        }}
      </For>
    ),
    "ul",
  );
  const first = el.firstChild;
  setItems(["c", "a"]);
  flush();
  expect(el.innerHTML).toBe("<li>0:c</li><li>1:a</li>");
  expect(el.firstChild).toBe(first);
  expect(created).toBe(3);
  setItems([]);
  flush();
  expect(el.innerHTML).toBe("");
});

test("For keyed={fn} is key={fn}", () => {
  const [items, setItems] = signal([{ id: 1, label: "one" }]);
  const { el } = mount(
    () => (
      <For each={items()} keyed={(row: { id: number; label: string }) => row.id}>
        {(row) => <li>{row().label}</li>}
      </For>
    ),
    "ul",
  );
  const li = el.firstChild;
  setItems([{ id: 1, label: "uno" }]);
  flush();
  expect(el.innerHTML).toBe("<li>uno</li>");
  expect(el.firstChild).toBe(li);
});

test("Repeat renders count rows from `from`, reusing rows whose index stays", () => {
  const [count, setCount] = signal(3);
  const [from, setFrom] = signal(0);
  const { el } = mount(
    () => (
      <Repeat count={count()} from={from()} fallback={<li>none</li>}>
        {(index) => <li>{index}</li>}
      </Repeat>
    ),
    "ul",
  );
  expect(el.innerHTML).toBe("<li>0</li><li>1</li><li>2</li>");
  const second = el.children[1];
  setFrom(1);
  flush();
  expect(el.innerHTML).toBe("<li>1</li><li>2</li><li>3</li>");
  expect(el.children[0]).toBe(second);
  setCount(0);
  flush();
  expect(el.innerHTML).toBe("<li>none</li>");
});

function slowCard() {
  const gates = new Map<string, PromiseWithResolvers<string>>();
  const gate = (name: string) => {
    if (!gates.has(name)) gates.set(name, Promise.withResolvers<string>());
    return gates.get(name)!;
  };
  async function Card(props: { name: string }) {
    const text = await gate(props.name).promise;
    return <b>{text}</b>;
  }
  return { Card, gate };
}

async function settle(): Promise<void> {
  await tick();
  flush();
}

test("Reveal sequential reveals in order; collapsed hides the tail fallbacks", async () => {
  const { Card, gate } = slowCard();
  const { el } = mount(() => (
    <Reveal collapsed>
      <Loading fallback={<i>a…</i>}>
        <Card name="a" />
      </Loading>
      <Loading fallback={<i>b…</i>}>
        <Card name="b" />
      </Loading>
      <Loading fallback={<i>c…</i>}>
        <Card name="c" />
      </Loading>
    </Reveal>
  ));
  expect(el.innerHTML).toBe("<i>a…</i>");
  gate("b").resolve("B");
  await settle();
  expect(el.innerHTML).toBe("<i>a…</i>");
  gate("a").resolve("A");
  await settle();
  expect(el.innerHTML).toBe("<b>A</b><b>B</b><i>c…</i>");
  gate("c").resolve("C");
  await settle();
  expect(el.innerHTML).toBe("<b>A</b><b>B</b><b>C</b>");
});

test("Reveal together waits for every boundary; natural reveals each on its own", async () => {
  const { Card, gate } = slowCard();
  const together = mount(() => (
    <Reveal order="together">
      <Loading fallback={<i>x…</i>}>
        <Card name="x" />
      </Loading>
      <Loading fallback={<i>y…</i>}>
        <Card name="y" />
      </Loading>
    </Reveal>
  ));
  const natural = mount(() => (
    <Reveal order="natural">
      <Loading fallback={<i>p…</i>}>
        <Card name="p" />
      </Loading>
      <Loading fallback={<i>q…</i>}>
        <Card name="q" />
      </Loading>
    </Reveal>
  ));
  gate("x").resolve("X");
  gate("q").resolve("Q");
  await settle();
  expect(together.el.innerHTML).toBe("<i>x…</i><i>y…</i>");
  expect(natural.el.innerHTML).toBe("<i>p…</i><b>Q</b>");
  gate("y").resolve("Y");
  await settle();
  expect(together.el.innerHTML).toBe("<b>X</b><b>Y</b>");
});

test("a nested Reveal is one slot of the enclosing one", async () => {
  const { Card, gate } = slowCard();
  const { el } = mount(() => (
    <Reveal>
      <Loading fallback={<i>head…</i>}>
        <Card name="head" />
      </Loading>
      <Reveal order="natural">
        <Loading fallback={<i>one…</i>}>
          <Card name="one" />
        </Loading>
        <Loading fallback={<i>two…</i>}>
          <Card name="two" />
        </Loading>
      </Reveal>
    </Reveal>
  ));
  gate("one").resolve("1");
  await settle();
  expect(el.innerHTML).toBe("<i>head…</i><i>one…</i><i>two…</i>");
  gate("head").resolve("H");
  await settle();
  expect(el.innerHTML).toBe("<b>H</b><b>1</b><i>two…</i>");
});
