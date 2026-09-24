import { flush, onCleanup, signal, store } from "@rezejs/signals";
import { cleanup, mount } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

import { For } from "../src/list";

afterEach(cleanup);

const texts = (el: Element) => [...el.children].map((c) => c.textContent);

test("rows follow items by identity: nodes move instead of being rebuilt", () => {
  const [items, setItems] = signal(["a", "b", "c"]);
  const { el } = mount(() => <For each={items()}>{(item) => <li>{item()}</li>}</For>, "ul");
  const [a, b, c] = el.children;
  setItems(["c", "a", "b"]);
  flush();
  expect(texts(el)).toEqual(["c", "a", "b"]);
  expect([...el.children]).toEqual([c, a, b]);
});

test("with key, a kept row updates item() in place", () => {
  type Row = { id: number; label: string };
  const [items, setItems] = signal<Row[]>([{ id: 1, label: "one" }]);
  const { el } = mount(
    () => (
      <For each={items()} key={(r) => r.id}>
        {(row) => <li>{row().label}</li>}
      </For>
    ),
    "ul",
  );
  const li = el.firstChild;
  setItems([{ id: 1, label: "uno" }]);
  flush();
  expect(texts(el)).toEqual(["uno"]);
  expect(el.firstChild).toBe(li);
});

test("index() tracks the row position", () => {
  const [items, setItems] = signal(["a", "b"]);
  const { el } = mount(
    () => (
      <For each={items()}>
        {(item, index) => (
          <li>
            {index()}:{item()}
          </li>
        )}
      </For>
    ),
    "ul",
  );
  setItems(["b", "a"]);
  flush();
  expect(texts(el)).toEqual(["0:b", "1:a"]);
});

test("removed rows are disposed; fallback shows for an empty list", () => {
  const log: string[] = [];
  const [items, setItems] = signal(["a", "b"]);
  const { el } = mount(
    () => (
      <For each={items()} fallback={<li>empty</li>}>
        {(item) => {
          onCleanup(() => log.push(item()));
          return <li>{item()}</li>;
        }}
      </For>
    ),
    "ul",
  );
  setItems(["b"]);
  flush();
  expect(log).toEqual(["a"]);
  setItems([]);
  flush();
  expect(log).toEqual(["a", "b"]);
  expect(texts(el)).toEqual(["empty"]);
  setItems(["c"]);
  flush();
  expect(texts(el)).toEqual(["c"]);
});

test("duplicate items map to distinct rows", () => {
  const [items, setItems] = signal(["x", "x", "y"]);
  const { el } = mount(() => <For each={items()}>{(item) => <li>{item()}</li>}</For>, "ul");
  setItems(["y", "x", "x", "x"]);
  flush();
  expect(texts(el)).toEqual(["y", "x", "x", "x"]);
  expect(new Set(el.children).size).toBe(4);
});

test("a store array mutated in place updates the rows and keeps the surviving ones", () => {
  const init = { todos: ["a", "b", "c"] };
  const [state, setState] = store(init);
  const { el } = mount(() => <For each={state.todos}>{(item) => <li>{item()}</li>}</For>, "ul");
  const [a, b, c] = el.children;
  setState((d) => {
    d.todos.push("d");
  });
  flush();
  expect(texts(el)).toEqual(["a", "b", "c", "d"]);
  setState((d) => {
    d.todos.splice(1, 1);
  });
  flush();
  expect(texts(el)).toEqual(["a", "c", "d"]);
  expect([...el.children].slice(0, 2)).toEqual([a, c]);
  setState((d) => {
    d.todos.reverse();
  });
  flush();
  expect(texts(el)).toEqual(["d", "c", "a"]);
  setState((d) => {
    d.todos[0] = "e";
  });
  flush();
  expect(texts(el)).toEqual(["e", "c", "a"]);
  expect([...el.children].slice(1)).toEqual([c, a]);
  expect(el.children).not.toContain(b);
});

test("random reorders, inserts and removals keep DOM order and reuse surviving nodes", () => {
  let seed = 7;
  const rand = (n: number) => (seed = (seed * 1103515245 + 12345) % 2 ** 31) % n;
  let next = 0;
  const [items, setItems] = signal<number[]>([]);
  const { el } = mount(
    () => (
      <>
        <li>head</li>
        <For each={items()}>{(item) => <li>{item()}</li>}</For>
        <li>tail</li>
      </>
    ),
    "ul",
  );
  let current: number[] = [];
  for (let step = 0; step < 300; step++) {
    const list = current.filter(() => rand(4) !== 0);
    for (let i = list.length; i > 1; i--) {
      if (rand(3) === 0) {
        const j = rand(i);
        [list[i - 1], list[j]] = [list[j]!, list[i - 1]!];
      }
    }
    for (let k = rand(5); k--;) list.splice(rand(list.length + 1), 0, next++);
    const before = new Map([...el.children].map((node) => [node.textContent, node]));
    setItems(list);
    flush();
    expect(texts(el)).toEqual(["head", ...list.map(String), "tail"]);
    for (const node of el.children) {
      const old = before.get(node.textContent);
      if (old && current.includes(Number(node.textContent))) expect(node).toBe(old);
    }
    current = list;
  }
});
