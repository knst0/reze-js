import { flush, onCleanup } from "@rezejs/signals";
import { expect, test } from "vite-plus/test";

import { render } from "../src/component";
import { hotComponent } from "../src/hot";

test("registering an id again re-renders mounted instances in place", () => {
  const cleaned: string[] = [];
  const V1 = (props: { label: string }) => {
    onCleanup(() => cleaned.push("v1"));
    const el = document.createElement("p");
    el.textContent = `v1 ${props.label}`;
    return el;
  };
  const V2 = (props: { label: string }) => {
    const el = document.createElement("p");
    el.textContent = `v2 ${props.label}`;
    return el;
  };

  const Hot = hotComponent({}, "b.tsx#Label", V1);
  const container = document.createElement("div");
  const dispose = render(() => Hot({ label: "x" }), container);
  expect(container.innerHTML).toBe("<p>v1 x</p>");

  expect(hotComponent({}, "b.tsx#Label", V2)).toBe(Hot);
  flush();
  expect(container.innerHTML).toBe("<p>v2 x</p>");
  expect(cleaned).toEqual(["v1"]);
  dispose();
});
