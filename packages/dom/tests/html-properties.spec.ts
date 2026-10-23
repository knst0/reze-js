import { expect, test } from "vite-plus/test";

import { applySpread } from "../src/html/properties";
import { serializeNodes } from "../src/html/serialize";
import { createElement } from "../src/html/tree";

const context = { tokenOf: () => "" };

test("a spread innerHTML on an SVG element renders markup, not an attribute", () => {
  const g = createElement("g", "svg");
  applySpread(g, { innerHTML: '<circle r="1"></circle>' });
  expect(serializeNodes([g], context)).toBe('<g><circle r="1"></circle></g>');
});
