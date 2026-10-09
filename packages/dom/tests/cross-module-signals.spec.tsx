import { cleanup, mount, tick } from "@rezejs/testing-library";
import { afterEach, expect, test } from "vitest";

import { View } from "./fixtures/cross-module/view";
afterEach(cleanup);
test("an importer reads an imported signal through named, namespace and function bindings", () => {
  const { el } = mount(() => <View />);
  expect(el.innerHTML).toBe("<button><p>0</p><i>0</i><b>0</b></button>");
  el.querySelector("button")!.click();
  tick();
  expect(el.innerHTML).toBe("<button><p>1</p><i>2</i><b>1</b></button>");
  el.querySelector("button")!.click();
  tick();
  expect(el.innerHTML).toBe("<button><p>2</p><i>4</i><b>2</b></button>");
});
