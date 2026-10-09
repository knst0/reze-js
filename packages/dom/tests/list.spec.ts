import { signal } from "@rezejs/signals";
import { setProfileHook, type ProfileNodeKind } from "@rezejs/signals/profile";
import { afterEach, expect, test } from "vite-plus/test";

import { render } from "../src/component";
import { list } from "../src/list";

let unmount: (() => void) | undefined;

afterEach(() => {
  unmount?.();
  unmount = undefined;
  setProfileHook(undefined);
});

function mount(code: () => unknown): HTMLElement {
  const container = document.createElement("ul");
  unmount = render(code as () => Node, container);
  return container;
}

test("rows create a signal for index() only when map declares it", () => {
  const [items] = signal(["a", "b", "c"]);
  const created: ProfileNodeKind[] = [];
  setProfileHook({
    event: ({ type, kind }) => {
      if (type === "created") created.push(kind);
    },
  });

  mount(() => list(items, (item) => document.createTextNode(item)));
  expect(created.filter((kind) => kind === "signal")).toEqual([]);

  unmount!();
  created.length = 0;
  mount(() => list(items, (item, index) => document.createTextNode(item + index())));
  expect(created.filter((kind) => kind === "signal")).toHaveLength(3);
});
