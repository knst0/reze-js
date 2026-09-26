import { flush, onCleanup, signal } from "@rezejs/signals";
import { setDebugHook, type DebugNodeKind } from "@rezejs/signals/devtools";
import { renderEffect } from "@rezejs/signals/render";
import { afterEach, expect, test } from "vitest";

import { render } from "../src/component";
import { list } from "../src/list";

let unmount: (() => void) | undefined;

afterEach(() => {
  unmount?.();
  unmount = undefined;
  setDebugHook(undefined);
});

function mount(code: () => unknown): HTMLElement {
  const container = document.createElement("ul");
  unmount = render(code as () => Node, container);
  return container;
}

function li(text: () => unknown): HTMLLIElement {
  const el = document.createElement("li");
  renderEffect(() => {
    el.textContent = String(text());
  });
  return el;
}

const texts = (el: Element) => [...el.children].map((child) => child.textContent);

test("rows keep their nodes across a reorder", () => {
  const [items, setItems] = signal(["a", "b", "c"]);
  const container = mount(() => list(items, (item) => li(item)));
  const [a, b, c] = container.children;
  setItems(["c", "a", "b"]);
  flush();
  expect(texts(container)).toEqual(["c", "a", "b"]);
  expect([...container.children]).toEqual([c, a, b]);
});

test("with key, a kept row keeps its node and updates item()", () => {
  const [items, setItems] = signal([
    { id: 1, label: "one" },
    { id: 2, label: "two" },
  ]);
  const container = mount(() =>
    list(
      items,
      (item) => li(() => item().label),
      undefined,
      (item) => item.id,
    ),
  );
  const [first, second] = container.children;
  setItems([
    { id: 2, label: "dos" },
    { id: 1, label: "uno" },
  ]);
  flush();
  expect(texts(container)).toEqual(["dos", "uno"]);
  expect([...container.children]).toEqual([second, first]);
});

test("removed rows run their cleanups", () => {
  const [items, setItems] = signal(["a", "b", "c"]);
  const cleaned: string[] = [];
  mount(() =>
    list(items, (item) => {
      onCleanup(() => cleaned.push(item()));
      return li(item);
    }),
  );
  setItems(["c", "a"]);
  flush();
  expect(cleaned).toEqual(["b"]);
  setItems(["a"]);
  flush();
  expect(cleaned).toEqual(["b", "c"]);
  unmount!();
  unmount = undefined;
  expect(cleaned).toEqual(["b", "c", "a"]);
});

test("the fallback shows while the list is empty and is disposed when rows arrive", () => {
  const [items, setItems] = signal<string[]>([]);
  let fallbacks = 0;
  let fallbackCleanups = 0;
  const container = mount(() =>
    list(
      items,
      (item) => li(item),
      () => {
        fallbacks++;
        onCleanup(() => fallbackCleanups++);
        return li(() => "empty");
      },
    ),
  );
  expect(texts(container)).toEqual(["empty"]);

  setItems(["a"]);
  flush();
  expect(texts(container)).toEqual(["a"]);
  expect(fallbackCleanups).toBe(1);

  setItems([]);
  flush();
  expect(texts(container)).toEqual(["empty"]);
  expect(fallbacks).toBe(2);
});

test("index() follows a moved row", () => {
  const [items, setItems] = signal(["a", "b", "c"]);
  const container = mount(() => list(items, (item, index) => li(() => `${index()}:${item()}`)));
  const [a, , c] = container.children;
  setItems(["c", "b", "a"]);
  flush();
  expect(texts(container)).toEqual(["0:c", "1:b", "2:a"]);
  expect(container.firstChild).toBe(c);
  expect(container.lastChild).toBe(a);
});

test("rows create a signal for index() only when map declares it", () => {
  const [items] = signal(["a", "b", "c"]);
  const created: DebugNodeKind[] = [];
  setDebugHook({
    created: (_, kind) => void created.push(kind),
    rerunning: () => {},
    disposed: () => {},
    written: () => {},
    component: (_, run) => run(),
  });

  mount(() => list(items, (item) => document.createTextNode(item())));
  expect(created.filter((kind) => kind === "signal")).toEqual([]);

  unmount!();
  created.length = 0;
  mount(() => list(items, (item, index) => document.createTextNode(item() + index())));
  expect(created.filter((kind) => kind === "signal")).toHaveLength(3);
});
