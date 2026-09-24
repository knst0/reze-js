import { computed, render, signal } from "@rezejs/dom";
import { Show } from "@rezejs/dom/flow";
import { flush } from "@rezejs/signals";
import { afterEach, expect, test } from "vitest";

import { installDevtools, type Devtools, type OwnerTreeNode, type SignalWrite } from "../src";

let devtools: Devtools | undefined;
let disposeApp: (() => void) | undefined;

afterEach(() => {
  disposeApp?.();
  devtools?.uninstall();
  document.body.innerHTML = "";
});

function Counter(props: { step: number }) {
  const [count, setCount] = signal(0);
  const doubled = computed(() => count() * 2);
  return (
    <section>
      <output>{count()}</output>
      <p>doubled: {doubled()}</p>
      <button onClick={() => setCount((n) => n + props.step)}>+{props.step}</button>
      <Show when={count() >= 2}>
        <Note />
      </Show>
    </section>
  );
}

function Note() {
  return <p class="note">many</p>;
}

function App() {
  return <Counter step={1} />;
}

function mountApp(): HTMLElement {
  const container = document.createElement("div");
  document.body.append(container);
  disposeApp = render(() => <App />, container);
  return container;
}

test("the owner tree groups nodes under the components that created them", () => {
  devtools = installDevtools();
  mountApp();
  expect(devtools.printOwnerTree()).toMatchInlineSnapshot(`
    "root
      App
        Counter
          signal count
          render
          computed computed#1
          render ×2"
  `);
});

function findNode(nodes: OwnerTreeNode[], name: string): OwnerTreeNode | undefined {
  for (const node of nodes) {
    if (node.name === name) return node;
    const found = findNode(node.children, name);
    if (found !== undefined) return found;
  }
  return undefined;
}

test("values, writes and the component that rendered an element", () => {
  devtools = installDevtools();
  const container = mountApp();
  const count = findNode(devtools.getOwnerTree(), "count")!;
  expect(devtools.getValue(count.id)).toBe(0);
  const writes: SignalWrite[] = [];
  const unsubscribe = devtools.subscribe((write) => writes.push(write));
  container.querySelector("button")!.click();
  flush();
  unsubscribe();
  container.querySelector("button")!.click();
  flush();
  expect(writes).toEqual([{ id: count.id, name: "count", value: 1 }]);
  expect(devtools.getValue(count.id)).toBe(2);
  expect(devtools.highlight(container.querySelector("output")!)?.name).toBe("Counter");
  expect(devtools.highlight(container.querySelector(".note")!)?.name).toBe("Note");
  expect(window.__REZE_DEVTOOLS__).toBe(devtools);
});

test("disposed owners and finished branches leave the tree", () => {
  devtools = installDevtools();
  const container = mountApp();
  expect(findNode(devtools.getOwnerTree(), "Note")).toBeUndefined();
  container.querySelector("button")!.click();
  container.querySelector("button")!.click();
  flush();
  expect(findNode(devtools.getOwnerTree(), "Note")).toBeDefined();
  disposeApp!();
  disposeApp = undefined;
  expect(devtools.getOwnerTree()).toEqual([]);
});

test("uninstall stops recording", () => {
  devtools = installDevtools();
  devtools.uninstall();
  mountApp();
  expect(devtools.getOwnerTree()).toEqual([]);
  expect(window.__REZE_DEVTOOLS__).toBeUndefined();
});
