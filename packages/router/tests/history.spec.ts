import { afterEach, expect, test, vi } from "vite-plus/test";

import { createBrowserHistory, createHashHistory } from "../src";

afterEach(() => window.history.replaceState(null, "", "/"));

function popTo(url: string): void {
  window.history.pushState(null, "", url);
  window.dispatchEvent(new PopStateEvent("popstate", { state: null }));
}

test("hash history leaves in-page anchors to the browser and keeps its route", () => {
  window.history.replaceState(null, "", "/#/a");
  const history = createHashHistory();
  const listener = vi.fn();
  history.listen(listener);
  popTo("#section");
  expect(listener).not.toHaveBeenCalled();
  expect(history.get().path).toBe("/a");
  popTo("#/b");
  expect(listener).toHaveBeenCalledWith(expect.objectContaining({ path: "/b" }));
});

test("a relative base is the root, as the generated href types assume", () => {
  window.history.replaceState(null, "", "/blog?x=1");
  expect(createBrowserHistory("./").get().path).toBe("/blog?x=1");
  expect(createBrowserHistory("/app/").resolve(new URL("http://localhost/app/blog"))).toBe("/blog");
});
