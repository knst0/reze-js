import { expect, test } from "vite-plus/test";

import { FlagDirty, FlagMutable, FlagNone, FlagPending } from "../src/flags";
import { checkDirty, link, type ReactiveNode } from "../src/graph";

function pending(dep: ReactiveNode, update: () => boolean): ReactiveNode {
  const node: ReactiveNode = {
    flags: FlagMutable | FlagPending,
    update() {
      node.flags = FlagMutable;
      return update();
    },
  };
  link(dep, node, 1);
  return node;
}

test("a nested dirty check preserves outer descents and resumes the outer sibling", () => {
  const order: string[] = [];
  const innerLeaf: ReactiveNode = {
    flags: FlagMutable | FlagDirty,
    update() {
      innerLeaf.flags = FlagMutable;
      order.push("inner-leaf");
      return true;
    },
  };
  const innerBranch = pending(innerLeaf, () => {
    order.push("inner-branch");
    return true;
  });
  const innerMiddle = pending(innerBranch, () => {
    order.push("inner-middle");
    return true;
  });
  const innerRoot = pending(innerMiddle, () => true);
  const outerLeaf: ReactiveNode = {
    flags: FlagMutable | FlagDirty,
    update() {
      outerLeaf.flags = FlagMutable;
      order.push("outer-leaf");
      expect(checkDirty(innerRoot.deps!, innerRoot)).toBe(true);
      return false;
    },
  };
  const outerBranch = pending(outerLeaf, () => {
    throw new Error("unchanged outer branch must not update");
  });
  const outerMiddle = pending(outerBranch, () => {
    throw new Error("unchanged outer middle must not update");
  });
  const outerRoot = pending(outerMiddle, () => true);
  const outerSibling: ReactiveNode = {
    flags: FlagMutable | FlagDirty,
    update() {
      outerSibling.flags = FlagMutable;
      order.push("outer-sibling");
      return true;
    },
  };
  link(outerSibling, outerRoot, 1);

  expect(checkDirty(outerRoot.deps!, outerRoot)).toBe(true);
  expect(order).toEqual(["outer-leaf", "inner-leaf", "inner-branch", "inner-middle", "outer-sibling"]);
  expect(outerBranch.flags).toBe(FlagMutable);
  expect(outerMiddle.flags).toBe(FlagMutable);
});

test("a dependency update disposing the checked root suppresses its dirty result", () => {
  const root: ReactiveNode = { flags: FlagMutable | FlagPending };
  const leaf: ReactiveNode = {
    flags: FlagMutable | FlagDirty,
    update() {
      leaf.flags = FlagMutable;
      root.flags = FlagNone;
      return true;
    },
  };
  link(leaf, root, 1);

  expect(checkDirty(root.deps!, root)).toBe(false);
  expect(root.flags).toBe(FlagNone);
});
