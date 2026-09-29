import { signal } from "@rezejs/signals";
import { setDebugHook, type DebugNodeKind } from "@rezejs/signals/devtools";
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
