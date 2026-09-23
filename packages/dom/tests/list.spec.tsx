import { onCleanup, signal } from "@rezejs/signals";
import { afterEach, expect, test } from "vitest";

import { render } from "../src";
import { For } from "../src/list";

let dispose: (() => void) | undefined;
function mount(code: () => unknown): HTMLElement {
  const el = document.createElement("ul");
  document.body.appendChild(el);
  dispose = render(code as () => Element, el);
  return el;
}
afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.textContent = "";
});

const texts = (el: Element) => [...el.children].map((c) => c.textContent);

test("rows follow items by identity: nodes move instead of being rebuilt", () => {
  const [items, setItems] = signal(["a", "b", "c"]);
  const el = mount(() => <For each={items()}>{(item) => <li>{item()}</li>}</For>);
  const [a, b, c] = el.children;
  setItems(["c", "a", "b"]);
  expect(texts(el)).toEqual(["c", "a", "b"]);
  expect([...el.children]).toEqual([c, a, b]);
});

test("with key, a kept row updates item() in place", () => {
  type Row = { id: number; label: string };
  const [items, setItems] = signal<Row[]>([{ id: 1, label: "one" }]);
  const el = mount(() => (
    <For each={items()} key={(r) => r.id}>
      {(row) => <li>{row().label}</li>}
    </For>
  ));
  const li = el.firstChild;
  setItems([{ id: 1, label: "uno" }]);
  expect(texts(el)).toEqual(["uno"]);
  expect(el.firstChild).toBe(li);
});

test("index() tracks the row position", () => {
  const [items, setItems] = signal(["a", "b"]);
  const el = mount(() => (
    <For each={items()}>
      {(item, index) => (
        <li>
          {index()}:{item()}
        </li>
      )}
    </For>
  ));
  setItems(["b", "a"]);
  expect(texts(el)).toEqual(["0:b", "1:a"]);
});

test("removed rows are disposed; fallback shows for an empty list", () => {
  const log: string[] = [];
  const [items, setItems] = signal(["a", "b"]);
  const el = mount(() => (
    <For each={items()} fallback={<li>empty</li>}>
      {(item) => {
        onCleanup(() => log.push(item()));
        return <li>{item()}</li>;
      }}
    </For>
  ));
  setItems(["b"]);
  expect(log).toEqual(["a"]);
  setItems([]);
  expect(log).toEqual(["a", "b"]);
  expect(texts(el)).toEqual(["empty"]);
  setItems(["c"]);
  expect(texts(el)).toEqual(["c"]);
});

test("duplicate items map to distinct rows", () => {
  const [items, setItems] = signal(["x", "x", "y"]);
  const el = mount(() => <For each={items()}>{(item) => <li>{item()}</li>}</For>);
  setItems(["y", "x", "x", "x"]);
  expect(texts(el)).toEqual(["y", "x", "x", "x"]);
  expect(new Set(el.children).size).toBe(4);
});

test("random reorders, inserts and removals keep DOM order and reuse surviving nodes", () => {
  let seed = 7;
  const rand = (n: number) => (seed = (seed * 1103515245 + 12345) % 2 ** 31) % n;
  let next = 0;
  const [items, setItems] = signal<number[]>([]);
  const el = mount(() => (
    <>
      <li>head</li>
      <For each={items()}>{(item) => <li>{item()}</li>}</For>
      <li>tail</li>
    </>
  ));
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
    expect(texts(el)).toEqual(["head", ...list.map(String), "tail"]);
    for (const node of el.children) {
      const old = before.get(node.textContent);
      if (old && current.includes(Number(node.textContent))) expect(node).toBe(old);
    }
    current = list;
  }
});
