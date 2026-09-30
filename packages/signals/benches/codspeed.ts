import { withCodSpeed } from "@codspeed/tinybench-plugin";
import {
  asyncComputed,
  boundary,
  computed,
  effect,
  effectScope,
  flush,
  getOwner,
  provideContext,
  root,
  runWithOwner,
  selector,
  signal,
  store,
  trigger,
  useContext,
  type ContextKey,
} from "@rezejs/signals";
import { Bench, type FnOptions } from "tinybench";

const CREATE_COUNT = 10_000;
const WRITE_ITERS = 1000;
const FANOUT_EFFECTS = 100;
const FANOUT_ITERS = 100;

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

bench.add("create: signals", () => {
  root((dispose) => {
    for (let i = 0; i < CREATE_COUNT; i++) signal(i);
    dispose();
  });
});

bench.add("create: signal + computed + effect", () => {
  root((dispose) => {
    for (let i = 0; i < CREATE_COUNT; i++) {
      const [get] = signal(i);
      const doubled = computed(() => get() * 2);
      effect(() => void doubled());
    }
    dispose();
  });
});

bench.add("create: row roots with a signal", () => {
  const disposers: (() => void)[] = [];
  for (let i = 0; i < CREATE_COUNT; i++) {
    root((dispose) => {
      disposers.push(dispose);
      const [get] = signal(i);
      effect(() => void get());
    });
  }
  for (const dispose of disposers) dispose();
});

{
  const [get, set] = signal(0);
  let next = 0;
  bench.add("raw signal: write + read, no subscribers", () => {
    for (let i = 0; i < WRITE_ITERS; i++) {
      next += 1;
      set(next);
      get();
    }
  });
}

{
  let setCount!: (value: number) => void;
  let next = 0;
  const hooks = withGraph(() => {
    const [count, set] = signal(0);
    setCount = set;
    const doubled = computed(() => count() * 2);
    effect(() => {
      count();
      doubled();
    });
  });
  bench.add(
    "counter path: click-like writes",
    () => {
      for (let i = 0; i < WRITE_ITERS; i++) {
        next += 1;
        setCount(next);
        flush();
      }
    },
    hooks,
  );
  bench.add(
    "counter path: coalesced writes",
    () => {
      for (let i = 0; i < WRITE_ITERS; i++) {
        next += 1;
        setCount(next);
      }
      flush();
    },
    hooks,
  );
}

{
  let set!: (value: number) => void;
  let next = 0;
  bench.add(
    `fanout: 1 write -> ${FANOUT_EFFECTS} effects`,
    () => {
      for (let i = 0; i < FANOUT_ITERS; i++) {
        next += 1;
        set(next);
        flush();
      }
    },
    withGraph(() => {
      const [get, setSource] = signal(0);
      set = setSource;
      for (let i = 0; i < FANOUT_EFFECTS; i++) {
        effect(() => {
          get();
        });
      }
    }),
  );
}

{
  interface State {
    user: { name: string; age: number };
    tags: string[];
  }
  let state!: State;
  let next = 0;
  const hooks = withGraph(() => {
    state = store({ user: { name: "a", age: 1 }, tags: ["x", "y"] });
    effect(() => void state.user.name);
    effect(() => void state.user.age);
    effect(() => void state.tags.length);
  });
  bench.add(
    "store: nested leaf writes with effects",
    () => {
      for (let i = 0; i < WRITE_ITERS; i++) {
        next += 1;
        state.user.name = `n${next}`;
        flush();
      }
    },
    hooks,
  );
}

{
  const ROWS = 1000;
  const MOVES = 60;
  let moveSelection!: (value: number) => void;
  const hooks = withGraph(() => {
    const [selected, setSelected] = signal(0);
    moveSelection = setSelected;
    const isSelected = selector(selected);
    for (let key = 0; key < ROWS; key++) {
      effect(() => void isSelected(key));
    }
  });
  let cursor = 0;
  bench.add(
    `selector: move selection across ${ROWS} rows`,
    () => {
      for (let i = 0; i < MOVES; i++) {
        cursor = (cursor + 1) % ROWS;
        moveSelection(cursor);
        flush();
      }
    },
    hooks,
  );
}

{
  const SETTLED_COUNT = 200;
  const SETTLED_READS = 16000;
  let settledReads: (() => number | undefined)[] = [];
  let disposeSettled: (() => void) | undefined;
  bench.add(
    "asyncComputed: settled value reads",
    () => {
      for (let i = 0; i < SETTLED_READS; i++) {
        settledReads[i % SETTLED_COUNT]!();
      }
    },
    {
      beforeAll: async () => {
        root((dispose) => {
          disposeSettled = dispose;
          settledReads = [];
          for (let i = 0; i < SETTLED_COUNT; i++) {
            const data = asyncComputed(() => Promise.resolve(i));
            settledReads.push(() => data.value());
          }
        });
        await Promise.resolve();
        await Promise.resolve();
        await Promise.resolve();
        flush();
      },
      afterAll() {
        disposeSettled?.();
        disposeSettled = undefined;
        settledReads = [];
      },
    },
  );
}

{
  const DEPTH = 32;
  const LOOKUPS = 1000;
  const chain: ContextKey<number>[] = Array.from({ length: DEPTH + 1 }, (_, index) => ({
    id: Symbol(`chain${index}`),
    defaultValue: -1,
  }));
  let runLookups!: (count: number) => void;
  const hooks = withGraph(() => {
    const nest = (depth: number): void => {
      if (depth === 0) {
        const owner = getOwner();
        runLookups = (count: number) => {
          runWithOwner(owner, () => {
            let found = 0;
            for (let i = 0; i < count; i++) {
              found += useContext(chain[0]!);
            }
            if (found !== count * 42) {
              throw new Error("context lookup missed its provider");
            }
          });
        };
        return;
      }
      provideContext(chain[depth]!, depth, () => nest(depth - 1));
    };
    provideContext(chain[0]!, 42, () => nest(DEPTH));
  });
  bench.add(
    `provide: lookups through ${DEPTH} owners`,
    () => {
      runLookups(LOOKUPS);
    },
    hooks,
  );
}

bench.add("create: effect scopes with signal + effect", () => {
  const SCOPES = 5000;
  const disposers: (() => void)[] = [];
  for (let i = 0; i < SCOPES; i++) {
    disposers.push(
      effectScope(() => {
        const [get] = signal(i);
        effect(() => void get());
      }),
    );
  }
  for (const dispose of disposers) dispose();
});

{
  const CELLS = 64;
  const TOUCHES = 300;
  let cells!: () => number[];
  const hooks = withGraph(() => {
    const [get] = signal(Array.from({ length: CELLS }, (_, index) => index));
    cells = get;
    const length = computed(() => cells().length);
    effect(() => void length());
  });
  let step = 0;
  bench.add(
    "trigger: in-place writes with manual notify",
    () => {
      for (let i = 0; i < TOUCHES; i++) {
        step += 1;
        cells()[step % CELLS] = step;
        trigger(cells);
      }
    },
    hooks,
  );
}

{
  const READERS = 200;
  const pending = asyncComputed(() => new Promise<number>(() => {}));
  bench.add(`boundary: register + release ${READERS} pending reads`, () => {
    root((dispose) => {
      const [, scope] = boundary(() => {
        for (let i = 0; i < READERS; i++) {
          effect(() => void pending.value());
        }
      });
      if (!scope.isPending()) {
        throw new Error("boundary is never pending");
      }
      dispose();
    });
  });
}

{
  const ROWS = 1000;
  const rows = store({ items: Array.from({ length: ROWS }, (_, v) => ({ v })) });
  bench.add("store: tracked reads of a list", () => {
    const dispose = effect(() => {
      const items = rows.items;
      for (let i = 0; i < items.length; i++) void items[i]!.v;
    });
    dispose();
  });
}

await bench.run();
console.table(bench.table());
