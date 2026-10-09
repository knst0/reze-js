import { withCodSpeed } from "@codspeed/tinybench-plugin";
import {
  asyncComponent,
  asyncViews,
  branch,
  choose,
  createComponent,
  list,
  mergeProps,
  reconcileArrays,
  repeat,
  splitProps,
} from "@rezejs/dom";
import { effect, flush, root, signal } from "@rezejs/signals";
import { Bench, type FnOptions } from "tinybench";

function withGraph(build: () => void): FnOptions {
  let dispose: (() => void) | undefined;
  return {
    beforeAll() {
      root((disposer) => {
        dispose = disposer;
        build();
      });
    },
    afterAll() {
      dispose?.();
    },
  };
}

const bench = withCodSpeed(new Bench());

{
  const TOGGLES = 60;
  let setShown!: (value: boolean) => void;
  const hooks = withGraph(() => {
    const [shown, set] = signal(true);
    setShown = set;
    const view = branch(
      shown,
      () => "on",
      () => "off",
    );
    effect(() => void view());
  });
  bench.add(
    "branch: toggle with effects reading the result",
    () => {
      for (let i = 0; i < TOGGLES; i++) {
        setShown(i % 2 === 0);
        flush();
      }
    },
    hooks,
  );
}
{
  const CHOICES = 4;
  const STEPS = 60;
  let pick!: (value: number) => void;
  const hooks = withGraph(() => {
    const [at, setAt] = signal(0);
    pick = setAt;
    const view = choose([() => at() === 0, () => at() === 1, () => at() === 2], [() => "zero", () => "one", () => "two"], () => "other");
    effect(() => void view());
  });
  bench.add(
    "choose: cycle the selected branch",
    () => {
      for (let i = 0; i < STEPS; i++) {
        pick(i % CHOICES);
        flush();
      }
    },
    hooks,
  );
}

{
  interface Item {
    id: number;
    label: string;
  }
  const ROWS = 200;
  const base: Item[] = Array.from({ length: ROWS }, (_, index) => ({ id: index, label: `row${index}` }));
  const grown: Item[] = [...base, { id: ROWS, label: `row${ROWS}` }];
  const shrunk: Item[] = base.slice(0, ROWS - 1);
  const reversed: Item[] = base.slice().reverse();
  const swapped: Item[] = base.slice();
  const first = swapped[0]!;
  swapped[0] = swapped[ROWS - 1]!;
  swapped[ROWS - 1] = first;
  let setItems!: (value: Item[]) => void;
  const hooks = withGraph(() => {
    const [items, set] = signal<Item[]>(base);
    setItems = set;
    const view = list(
      items,
      (item) => item().label,
      () => "empty",
      (item) => item.id,
    );
    effect(() => void view());
    flush();
  });
  let flipped = false;
  const alternate = (other: Item[], times: number): void => {
    for (let i = 0; i < times; i++) {
      flipped = !flipped;
      setItems(flipped ? other : base);
      flush();
    }
  };
  bench.add("list: append a row", () => alternate(grown, 10), hooks);
  bench.add("list: remove a row", () => alternate(shrunk, 10), hooks);
  bench.add("list: reverse the rows", () => alternate(reversed, 4), hooks);
  bench.add("list: swap two rows", () => alternate(swapped, 4), hooks);
}

{
  const LOW = 50;
  const HIGH = 60;
  let setCount!: (value: number) => void;
  const hooks = withGraph(() => {
    const [count, set] = signal(LOW);
    setCount = set;
    const view = repeat(
      count,
      (index) => `row${index}`,
      () => "empty",
    );
    effect(() => void view());
    flush();
  });
  let big = false;
  bench.add(
    "repeat: grow and shrink the row count",
    () => {
      for (let i = 0; i < 10; i++) {
        big = !big;
        setCount(big ? HIGH : LOW);
        flush();
      }
    },
    hooks,
  );
}

bench.add("loading: build with pending async children", () => {
  for (let i = 0; i < 12; i++) {
    root((dispose) => {
      const view = asyncViews(
        () => {
          const first = asyncComponent(
            () => new Promise<[string]>(() => {}),
            (values) => values()[0],
          );
          const second = asyncComponent(
            () => new Promise<[string]>(() => {}),
            (values) => values()[0],
          );
          return [first(), second()];
        },
        () => "spinner",
      );
      if (view() !== "spinner") {
        throw new Error("loading shows its fallback while children are pending");
      }
      dispose();
    });
  }
});

{
  const SETTLED_READS = 2000;
  let settledView!: () => string | undefined;
  let disposeSettled: (() => void) | undefined;
  bench.add(
    "asyncComponent: settled value reads",
    () => {
      for (let i = 0; i < SETTLED_READS; i++) {
        settledView();
      }
    },
    {
      beforeAll: async () => {
        root((dispose) => {
          disposeSettled = dispose;
          settledView = asyncComponent(
            () => Promise.resolve(["ready"] as [string]),
            (values) => values()[0],
          );
          effect(() => void settledView());
        });
        await Promise.resolve();
        await Promise.resolve();
        await Promise.resolve();
        flush();
      },
      afterAll() {
        disposeSettled?.();
        disposeSettled = undefined;
      },
    },
  );
}

{
  const KEYS = 24;
  const CYCLES = 50;
  const first: Record<string, unknown> = {};
  const second: Record<string, unknown> = {};
  for (let i = 0; i < KEYS; i++) {
    first[`k${i}`] = i;
    second[`k${i}`] = `v${i}`;
  }
  const renderProps = (props: Record<string, unknown>): string => {
    let out = "";
    for (let i = 0; i < KEYS; i++) {
      out += String(props[`k${i}`]);
    }
    return out;
  };
  let drain = "";
  bench.add("props: mergeProps + createComponent with many props", () => {
    for (let i = 0; i < CYCLES; i++) {
      drain = createComponent(renderProps, mergeProps(first, second)) as string;
    }
    if (drain.length === 0) {
      throw new Error("props render nothing");
    }
  });
  bench.add("props: splitProps with many keys", () => {
    const groupA = Array.from({ length: 8 }, (_, i) => `k${i}`);
    const groupB = Array.from({ length: 8 }, (_, i) => `k${i + 8}`);
    for (let i = 0; i < CYCLES; i++) {
      const [a, b, rest] = splitProps(second, groupA, groupB);
      drain = `${renderProps(a)}${renderProps(b)}${renderProps(rest)}`;
    }
    if (drain.length === 0) {
      throw new Error("props render nothing");
    }
  });
}

{
  const STEADY_READS = 12000;
  const RESET_CYCLES = 50;
  let steadyView!: () => unknown;
  const hooks = withGraph(() => {
    steadyView = asyncViews(
      () => "ok",
      undefined,
      () => "fallback",
    );
    effect(() => void steadyView());
  });
  bench.add(
    "asyncViews failure: steady reads without errors",
    () => {
      for (let i = 0; i < STEADY_READS; i++) {
        steadyView();
      }
    },
    hooks,
  );
  bench.add("asyncViews failure: throw + fallback + reset", () => {
    for (let i = 0; i < RESET_CYCLES; i++) {
      let fails = true;
      let reset!: () => void;
      root((dispose) => {
        const view = asyncViews(
          () => {
            if (fails) {
              throw new Error("boom");
            }
            return "recovered";
          },
          undefined,
          (_error, retry) => {
            reset = retry;
            return "fallback";
          },
        );
        if (view() !== "fallback") {
          throw new Error("asyncViews shows its failure view after a throw");
        }
        fails = false;
        reset();
        flush();
        const shown = view();
        if ((typeof shown === "function" ? shown() : shown) !== "recovered") {
          throw new Error("asyncViews shows children again after retry");
        }
        dispose();
      });
    }
  });
}

class FakeChild {
  parent: FakeParent | undefined = undefined;
  get nextSibling(): FakeChild | null {
    if (this.parent === undefined) {
      return null;
    }
    const siblings = this.parent.children;
    return siblings[siblings.indexOf(this) + 1] ?? null;
  }
  remove(): void {
    const parent = this.parent;
    if (parent === undefined) {
      return;
    }
    parent.children.splice(parent.children.indexOf(this), 1);
    this.parent = undefined;
  }
}

class FakeParent {
  children: FakeChild[] = [];
  insertBefore(node: FakeChild, anchor: FakeChild | null): void {
    node.remove();
    this.children.splice(anchor === null ? this.children.length : this.children.indexOf(anchor), 0, node);
    node.parent = this;
  }
  replaceChild(node: FakeChild, old: FakeChild): void {
    node.remove();
    this.children[this.children.indexOf(old)] = node;
    node.parent = this;
    old.parent = undefined;
  }
}

{
  const probe = new FakeParent();
  const nodes = [new FakeChild(), new FakeChild(), new FakeChild()];
  probe.children = nodes.slice();
  for (const node of probe.children) {
    node.parent = probe;
  }
  reconcileArrays(probe as unknown as Node, nodes.slice() as unknown as Node[], [nodes[2]!, nodes[0]!, nodes[1]!] as unknown as Node[]);
  const order = probe.children;
  if (order[0] !== nodes[2] || order[1] !== nodes[0] || order[2] !== nodes[1]) {
    throw new Error("fake nodes do not follow DOM move semantics");
  }
}

{
  const NODES = 200;
  const orderA: FakeChild[] = Array.from({ length: NODES }, () => new FakeChild());
  const orderB: FakeChild[] = orderA.slice().reverse();
  const parent = new FakeParent();
  parent.children = orderA.slice();
  for (const node of parent.children) {
    node.parent = parent;
  }
  let current: FakeChild[] = orderA;
  let flipped = false;
  bench.add(`reconcile: reorder ${NODES} nodes`, () => {
    flipped = !flipped;
    const target = flipped ? orderB : orderA;
    reconcileArrays(parent as unknown as Node, current.slice() as unknown as Node[], target as unknown as Node[]);
    current = target;
  });
}

await bench.run();
console.table(bench.table());
