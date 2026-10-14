import { settle } from "@rezejs/testing-library";
import { expect, test, vi } from "vite-plus/test";

import { action, computed, effect, flush, store, type Action, type Run } from "../src";

interface Todo {
  id: number;
  done: boolean;
  updatedAt?: number;
}

function gate<T = void>(): PromiseWithResolvers<T> {
  return Promise.withResolvers<T>();
}

function speculate(write: () => void, after: () => void = () => {}): Action<[wait: Promise<void>], void> {
  return action(async (run: Run, wait: Promise<void>) => {
    try {
      write();
      run.resume(await run.suspend(wait));
      after();
    } finally {
      run.end();
    }
  });
}

function values<T>(read: () => T): T[] {
  const seen: T[] = [];
  effect(() => {
    seen.push(read());
  });
  return seen;
}

interface Shape {
  user: { name?: string; tags?: string[] };
  list: string[];
  extra?: number;
}

const initial = (): Shape => ({ user: { name: "a" }, list: ["c", "a", "b"] });

test("writes made in an action are visible at once, kept on success and undone on failure", async () => {
  const writes: [string, (state: Shape) => void, Shape][] = [
    ["a property", (s) => (s.user.name = "z"), { user: { name: "z" }, list: ["c", "a", "b"] }],
    ["a nested object", (s) => (s.user = { name: "n", tags: ["t"] }), { user: { name: "n", tags: ["t"] }, list: ["c", "a", "b"] }],
    ["an added key", (s) => (s.extra = 1), { user: { name: "a" }, list: ["c", "a", "b"], extra: 1 }],
    ["a delete", (s) => delete s.user.name, { user: {}, list: ["c", "a", "b"] }],
    ["push", (s) => s.list.push("d", "e"), { user: { name: "a" }, list: ["c", "a", "b", "d", "e"] }],
    ["splice", (s) => s.list.splice(0, 2, "x"), { user: { name: "a" }, list: ["x", "b"] }],
    ["sort", (s) => s.list.sort(), { user: { name: "a" }, list: ["a", "b", "c"] }],
    ["length", (s) => (s.list.length = 1), { user: { name: "a" }, list: ["c"] }],
  ];
  for (const [label, write, written] of writes) {
    for (const outcome of ["resolve", "reject"] as const) {
      const state = store(initial());
      const joined = values(() => state.list.join());
      const wait = gate();
      const result = speculate(() => write(state))(wait.promise);
      expect(state, `${label}: visible before settle`).toEqual(written);
      wait[outcome]();
      await result.catch(() => {});
      const expected = outcome === "resolve" ? written : initial();
      expect(state, `${label}: ${outcome}`).toEqual(expected);
      expect(Object.keys(state.list), `${label}: ${outcome} keys`).toEqual(Object.keys(expected.list));
      flush();
      expect(joined.at(-1), `${label}: ${outcome} effect`).toBe(expected.list.join());
    }
  }
});

test("a failed action rejects with what it threw, and a synchronous action keeps writes or undoes them on throw", async () => {
  const state = store({ n: 0 });
  const failure = new Error("offline");
  const wait = gate();
  const result = speculate(() => state.n++)(wait.promise);
  wait.reject(failure);
  await expect(result).rejects.toBe(failure);
  expect(state.n).toBe(0);

  const sync = store({ n: 0 });
  const add = action((_run: Run, by: number) => {
    sync.n += by;
    if (by < 0) throw new RangeError("negative");
    return sync.n;
  });
  await expect(add(2)).resolves.toBe(2);
  const failing = add(-1);
  expect(sync.n).toBe(2);
  await expect(failing).rejects.toThrow(RangeError);
});

test("two runs toggling one key show each settled value once, whichever order they settle in", async () => {
  const settlementOrders: [string, [run: "a" | "b", outcome: "resolve" | "reject"][], boolean[]][] = [
    [
      "A fails while B is live",
      [
        ["a", "reject"],
        ["b", "resolve"],
      ],
      [false, true, false],
    ],
    [
      "B fails before A succeeds",
      [
        ["b", "reject"],
        ["a", "resolve"],
      ],
      [false, true, false, true],
    ],
    [
      "both succeed, A first",
      [
        ["a", "resolve"],
        ["b", "resolve"],
      ],
      [false, true, false],
    ],
    [
      "B succeeds, then A fails",
      [
        ["b", "resolve"],
        ["a", "reject"],
      ],
      [false, true, false],
    ],
  ];
  for (const [label, settlements, trace] of settlementOrders) {
    const todo = store<Todo>({ id: 1, done: false });
    const seen = values(() => todo.done);
    const gates = { a: gate(), b: gate() };
    const toggle = speculate(() => (todo.done = !todo.done));
    const runs = { a: toggle(gates.a.promise).catch(() => {}), b: undefined as Promise<void> | undefined };
    flush();
    runs.b = toggle(gates.b.promise).catch(() => {});
    flush();
    for (const [run, outcome] of settlements) {
      gates[run][outcome]();
      await runs[run];
      flush();
    }
    expect(seen, label).toEqual(trace);
  }
});

test("writes made outside an action land underneath it, and actions restore what they started from", async () => {
  const todo = store<Todo>({ id: 1, done: false });
  const failed = gate();
  const failing = speculate(() => (todo.done = true))(failed.promise).catch(() => {});
  todo.done = false;
  expect(todo.done).toBe(true);
  failed.reject();
  await failing;
  expect(todo.done).toBe(false);

  const succeeded = gate();
  const succeeding = speculate(() => (todo.done = true))(succeeded.promise);
  todo.done = false;
  expect(todo.done).toBe(true);
  succeeded.resolve();
  await succeeding;
  expect(todo.done).toBe(false);

  const state = store({ n: 0 });
  await speculate(() => (state.n = 1))(Promise.resolve());
  state.n = 2;
  expect(state.n).toBe(2);
  const wait = gate();
  const result = speculate(() => (state.n = 3))(wait.promise).catch(() => {});
  wait.reject();
  await result;
  expect(state.n).toBe(2);
});

test("an effect sees each settled value once, whether the action resolves, rejects, or is refetched before rejecting", async () => {
  const rows = [
    ["success", "resolve", false, [false, true]],
    ["failure", "reject", false, [false, true, false]],
    ["a refetch agreeing with the action, then failure", "reject", true, [false, true]],
  ] as const;
  for (const [label, outcome, refetch, trace] of rows) {
    const todo = store<Todo>({ id: 1, done: false });
    const seen = values(() => todo.done);
    const wait = gate();
    const result = speculate(() => (todo.done = !todo.done))(wait.promise).catch(() => {});
    flush();
    if (refetch) {
      todo.done = true;
      flush();
    }
    wait[outcome]();
    await result;
    flush();
    expect(seen, label).toEqual(trace);
  }
});

test("each run owns the writes it makes where it runs: late older responses, parallel runs, nested and deferred writes", async () => {
  const todo = store<Todo>({ id: 1, done: false });
  const seen = values(() => todo.done);
  const toggle = action(async (run: Run, wait: Promise<number>) => {
    try {
      todo.done = !todo.done;
      const updatedAt = run.resume(await run.suspend(wait));
      todo.updatedAt = updatedAt;
    } finally {
      run.end();
    }
  });
  const a = gate<number>();
  const b = gate<number>();
  const runA = toggle(a.promise);
  flush();
  const runB = toggle(b.promise);
  flush();
  a.resolve(1);
  await runA;
  flush();
  expect([todo.done, todo.updatedAt]).toEqual([false, 1]);
  b.resolve(2);
  await runB;
  flush();
  expect([todo.done, todo.updatedAt]).toEqual([false, 2]);
  expect(seen).toEqual([false, true, false]);

  const parallel = store({ a: 0, b: 0 });
  const first = gate();
  const second = gate();
  const firstRun = speculate(
    () => {},
    () => (parallel.a = 1),
  );
  const secondRun = action(async (run: Run, wait: Promise<void>) => {
    try {
      run.resume(await run.suspend(wait));
      parallel.b = 1;
      throw new Error("second");
    } finally {
      run.end();
    }
  });
  const runFirst = firstRun(first.promise);
  const runSecond = secondRun(second.promise).catch(() => {});
  second.resolve();
  first.resolve();
  await Promise.all([runFirst, runSecond]);
  expect(parallel).toEqual({ a: 1, b: 0 });

  const nested = store({ inner: 0, caught: 0, later: 0 });
  const inner = action((_run: Run) => {
    nested.inner = 1;
    throw new Error("inner");
  });
  const outer = action(async (run: Run, wait: Promise<void>) => {
    try {
      inner().catch(() => {});
      run.resume(await run.suspend(wait));
    } catch {
      run.resume();
      nested.caught = 1;
      setTimeout(() => (nested.later = 1));
      throw new Error("outer");
    } finally {
      run.end();
    }
  });
  const wait = gate();
  const result = outer(wait.promise).catch(() => {});
  expect(nested.inner).toBe(0);
  wait.reject();
  await result;
  await settle();
  expect(nested).toEqual({ inner: 0, caught: 0, later: 1 });
});

test("pending counts runs in flight and error holds the latest failure until the next call, both tracked", async () => {
  const save = speculate(() => {});
  const pending = values(() => save.pending);
  const errors = values(() => save.error);
  const a = gate();
  const b = gate();
  const failure = new Error("a");
  const runA = save(a.promise).catch(() => {});
  const runB = save(b.promise);
  flush();
  a.reject(failure);
  await runA;
  flush();
  b.resolve();
  await runB;
  flush();
  const c = gate();
  const runC = save(c.promise);
  flush();
  c.resolve();
  await runC;
  flush();
  expect(pending).toEqual([0, 2, 1, 0, 1, 0]);
  expect(errors).toEqual([undefined, failure, undefined]);
});

test("calling an action while a computed runs throws in development", () => {
  const save = action(() => {});
  const derived = computed(() => save());
  expect(derived).toThrow("action was called while a computed");
});

test("undoing a length change of an array also changed by an interfering write warns in development", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const interferences: [string, (list: string[]) => void][] = [
    ["another action in flight", (list) => void speculate(() => list.push("b"))(gate().promise).catch(() => {})],
    ["a plain write", (list) => void list.push("b")],
  ];
  for (const [label, interfere] of interferences) {
    warn.mockClear();
    const state = store({ list: [] as string[] });
    const wait = gate();
    const result = speculate(() => state.list.push("a"))(wait.promise).catch(() => {});
    interfere(state.list);
    wait.reject();
    await result;
    expect(warn, label).toHaveBeenCalledOnce();
    warn.mockClear();
    const alone = gate();
    const failing = speculate(() => state.list.push("c"))(alone.promise).catch(() => {});
    alone.reject();
    await failing;
    expect(warn, `${label}: alone`).not.toHaveBeenCalled();
  }
  warn.mockRestore();
});
