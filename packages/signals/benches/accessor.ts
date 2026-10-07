import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

import { Bench, type FnOptions } from "tinybench";

import { computed, effect, flush, root, signal } from "../src/index";
import { SignalNode } from "../src/signal";

type Accessor<T> = { (): T; (next: T | ((prev: T) => T)): T };
type Pair = [() => number, (next: number | ((prev: number) => number)) => number];

function writeNode<T>(node: SignalNode<T>, next: T | ((prev: T) => T)): T {
  const value = typeof next === "function" ? (next as (prev: T) => T)(node.pendingValue) : next;
  node.write(value);
  return value;
}

function boundOper<T>(this: SignalNode<T>, ...value: [] | [T | ((prev: T) => T)]): T {
  return value.length === 0 ? this.read() : writeNode(this, value[0]);
}

function boundSignal<T>(initial: T): Accessor<T> {
  return boundOper.bind(new SignalNode<T>(initial, Object.is) as SignalNode<unknown>) as unknown as Accessor<T>;
}

function closureSignal<T>(initial: T): Accessor<T> {
  const node = new SignalNode<T>(initial, Object.is);
  return function (next?: T | ((prev: T) => T)): T {
    return arguments.length === 0 ? node.read() : writeNode(node, next as T);
  } as Accessor<T>;
}

const variants = ["tuple", "bound", "closure"] as const;
type Variant = (typeof variants)[number];

const CREATE_COUNT = 10_000;
const MEMORY_COUNT = 100_000;
const WRITE_ITERS = 1000;
const FANOUT_EFFECTS = 100;
const FANOUT_ITERS = 100;
const WIDE_SIGNALS = 100;
const WIDE_ITERS = 100;

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

function factoriesOf(name: Variant): { create: (i: number) => unknown; pair: (value: number) => Pair } {
  if (name === "tuple") {
    return { create: (i) => signal(i), pair: (value) => signal(value) };
  }
  const make = name === "bound" ? boundSignal<number> : closureSignal<number>;
  return {
    create: (i) => make(i),
    pair: (value) => {
      const accessor = make(value);
      return [accessor, accessor];
    },
  };
}

function assertEquivalent(name: Variant, pair: (value: number) => Pair): void {
  let ok = false;
  root((dispose) => {
    const [read, write] = pair(1);
    let seen = 0;
    effect(() => {
      seen = read();
    });
    write(2);
    flush();
    ok = seen === 2 && read() === 2 && write((prev) => prev + 1) === 3;
    dispose();
  });
  if (!ok) throw new Error(`accessor variant ${name} is not equivalent`);
}

function measureMemory(name: Variant, create: (i: number) => unknown): void {
  if (globalThis.gc === undefined) throw new Error("run with --expose-gc");
  globalThis.gc();
  const before = process.memoryUsage().heapUsed;
  const kept: unknown[] = new Array(MEMORY_COUNT);
  for (let i = 0; i < MEMORY_COUNT; i++) kept[i] = create(i);
  globalThis.gc();
  const after = process.memoryUsage().heapUsed;
  console.log(`${name}: ${((after - before) / MEMORY_COUNT).toFixed(1)} bytes/signal`);
  if (kept.length !== MEMORY_COUNT) throw new Error("unreachable");
}

async function runChild(name: Variant): Promise<void> {
  const { create, pair } = factoriesOf(name);
  assertEquivalent(name, pair);
  measureMemory(name, create);

  const keep: unknown[] = new Array(CREATE_COUNT);
  let sum = 0;
  const bench = new Bench();

  bench.add("create 10000 signals", () => {
    for (let i = 0; i < CREATE_COUNT; i++) keep[i] = create(i);
  });

  {
    const [read] = pair(1);
    bench.add("untracked read x1000", () => {
      for (let i = 0; i < WRITE_ITERS; i++) sum += read();
    });
  }

  {
    const [read, write] = pair(0);
    let next = 0;
    bench.add("write+read, no subscribers x1000", () => {
      for (let i = 0; i < WRITE_ITERS; i++) {
        next += 1;
        write(next);
        read();
      }
    });
  }

  {
    let write!: Pair[1];
    let next = 0;
    const hooks = withGraph(() => {
      const [read, set] = pair(0);
      write = set;
      const doubled = computed(() => read() * 2);
      effect(() => {
        read();
        doubled();
      });
    });
    bench.add(
      "counter path: write+flush x1000",
      () => {
        for (let i = 0; i < WRITE_ITERS; i++) {
          next += 1;
          write(next);
          flush();
        }
      },
      hooks,
    );
  }

  {
    let write!: Pair[1];
    let next = 0;
    const hooks = withGraph(() => {
      const [read, set] = pair(0);
      write = set;
      for (let i = 0; i < FANOUT_EFFECTS; i++) {
        effect(() => {
          read();
        });
      }
    });
    bench.add(
      "fanout: 1 write -> 100 effects x100",
      () => {
        for (let i = 0; i < FANOUT_ITERS; i++) {
          next += 1;
          write(next);
          flush();
        }
      },
      hooks,
    );
  }

  {
    const writes: Pair[1][] = [];
    let next = 0;
    const hooks = withGraph(() => {
      writes.length = 0;
      const reads: Pair[0][] = [];
      for (let i = 0; i < WIDE_SIGNALS; i++) {
        const [read, write] = pair(i);
        reads.push(read);
        writes.push(write);
      }
      effect(() => {
        let total = 0;
        for (let i = 0; i < WIDE_SIGNALS; i++) total += reads[i]!();
        sum += total;
      });
    });
    bench.add(
      "effect reads 100 signals, write 1 + flush x100",
      () => {
        for (let i = 0; i < WIDE_ITERS; i++) {
          next += 1;
          writes[i % WIDE_SIGNALS]!(next);
          flush();
        }
      },
      hooks,
    );
  }

  await bench.run();
  console.log(`--- ${name} ---`);
  console.table(bench.table());
  if (sum === -1 || keep.length === -1) throw new Error("unreachable");
}

const requested = process.argv[2];
const child = variants.find((name) => name === requested);
if (child === undefined) {
  for (const name of variants) {
    execFileSync(process.execPath, ["--expose-gc", fileURLToPath(import.meta.url), name], { stdio: "inherit" });
  }
} else {
  await runChild(child);
}
