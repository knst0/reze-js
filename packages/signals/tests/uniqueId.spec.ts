import { expect, test } from "vitest";

import { createUniqueId, effect, root } from "../src";

test("ids are unique across separate roots, nested owners and no owner, and are valid CSS identifiers", () => {
  const ids = (): string[] =>
    root(() => {
      const found = [createUniqueId()];
      effect(() => {
        found.push(createUniqueId());
        root(() => {
          found.push(createUniqueId());
        });
      });
      return found;
    });
  const all = [...ids(), ...ids(), createUniqueId(), createUniqueId()];
  expect(new Set(all).size).toBe(all.length);
  expect(all.every((id) => /^[a-z][\w-]*$/i.test(id))).toBe(true);
});
