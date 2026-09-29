import { withCodSpeed } from "@codspeed/tinybench-plugin";
import { computed, effect, flush, root, signal } from "@rezejs/signals";
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

await bench.run();
console.table(bench.table());
