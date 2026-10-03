import { settle } from "@rezejs/testing-library";
import { expect, expectTypeOf, test, vi } from "vitest";

import { $action, action, computed, effect, flush, store, type Action, type Run } from "../src";

interface Todo {
  id: number;
  done: boolean;
  updatedAt?: number;
}

function gate<T = void>(): PromiseWithResolvers<T> {
  return Promise.withResolvers<T>();
}

/** What `$action` compiles `async (wait) => { write(); await wait; after(); }` to. */
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

test.each<[string, (state: Shape) => void, Shape]>([
  ["a property", (s) => (s.user.name = "z"), { user: { name: "z" }, list: ["c", "a", "b"] }],
  ["a nested object", (s) => (s.user = { name: "n", tags: ["t"] }), { user: { name: "n", tags: ["t"] }, list: ["c", "a", "b"] }],
  ["an added key", (s) => (s.extra = 1), { user: { name: "a" }, list: ["c", "a", "b"], extra: 1 }],
  ["a delete", (s) => delete s.user.name, { user: {}, list: ["c", "a", "b"] }],
  ["push", (s) => s.list.push("d", "e"), { user: { name: "a" }, list: ["c", "a", "b", "d", "e"] }],
  ["splice", (s) => s.list.splice(0, 2, "x"), { user: { name: "a" }, list: ["x", "b"] }],
  ["sort", (s) => s.list.sort(), { user: { name: "a" }, list: ["a", "b", "c"] }],
  ["length", (s) => (s.list.length = 1), { user: { name: "a" }, list: ["c"] }],
])("%s written in an action is visible at once, kept on success and undone on failure", async (_, write, written) => {
  for (const outcome of ["resolve", "reject"] as const) {
    const state = store(initial());
    const joined = values(() => state.list.join());
    const wait = gate();
    const result = speculate(() => write(state))(wait.promise);
    expect(state).toEqual(written);
    wait[outcome]();
    await result.catch(() => {});
    const expected = outcome === "resolve" ? written : initial();
    expect(state).toEqual(expected);
    expect(Object.keys(state.list)).toEqual(Object.keys(expected.list));
    flush();
    expect(joined.at(-1)).toBe(expected.list.join());
  }
});

test("a failed action rejects its promise with what it threw", async () => {
  const state = store({ n: 0 });
  const failure = new Error("offline");
  const wait = gate();
  const result = speculate(() => state.n++)(wait.promise);
  wait.reject(failure);
  await expect(result).rejects.toBe(failure);
  expect(state.n).toBe(0);
});

test.each<[string, [run: "a" | "b", outcome: "resolve" | "reject"][], boolean[]]>([
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
])("two runs toggling one key: %s", async (_, settlements, trace) => {
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
  expect(seen).toEqual(trace);
});

test("a write outside any action to a key an action holds lands underneath it", async () => {
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
});

test.each([
  ["success", "resolve", false, [false, true]],
  ["failure", "reject", false, [false, true, false]],
  ["a refetch agreeing with the action, then failure", "reject", true, [false, true]],
] as const)("an effect sees each value once: %s", async (_, outcome, refetch, trace) => {
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
  expect(seen).toEqual(trace);
});

test("a late response of an older run does not undo the newer run on the same key", async () => {
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
});

test("parallel runs each own the writes they make after their own awaits", async () => {
  const state = store({ a: 0, b: 0 });
  const a = gate();
  const b = gate();
  const first = speculate(
    () => {},
    () => (state.a = 1),
  );
  const second = action(async (run: Run, wait: Promise<void>) => {
    try {
      run.resume(await run.suspend(wait));
      state.b = 1;
      throw new Error("second");
    } finally {
      run.end();
    }
  });
  const runFirst = first(a.promise);
  const runSecond = second(b.promise).catch(() => {});
  b.resolve();
  a.resolve();
  await Promise.all([runFirst, runSecond]);
  expect(state).toEqual({ a: 1, b: 0 });
});

test("a nested action, a `catch` that resumes, and a deferred callback each write where they run", async () => {
  const state = store({ inner: 0, caught: 0, later: 0 });
  const inner = action((_run: Run) => {
    state.inner = 1;
    throw new Error("inner");
  });
  const outer = action(async (run: Run, wait: Promise<void>) => {
    try {
      inner().catch(() => {});
      run.resume(await run.suspend(wait));
    } catch {
      run.resume();
      state.caught = 1;
      setTimeout(() => (state.later = 1));
      throw new Error("outer");
    } finally {
      run.end();
    }
  });
  const wait = gate();
  const result = outer(wait.promise).catch(() => {});
  expect(state.inner).toBe(0);
  wait.reject();
  await result;
  await settle();
  expect(state).toEqual({ inner: 0, caught: 0, later: 1 });
});

test("a synchronous action keeps its writes on return and undoes them on a throw", async () => {
  const state = store({ n: 0 });
  const add = action((_run: Run, by: number) => {
    state.n += by;
    if (by < 0) throw new RangeError("negative");
    return state.n;
  });
  await expect(add(2)).resolves.toBe(2);
  const failing = add(-1);
  expect(state.n).toBe(2);
  await expect(failing).rejects.toThrow(RangeError);
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

test("writes outside actions stay real once actions exist, and an action restores the value it started from", async () => {
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

test("calling an action while a computed runs throws in development", () => {
  const save = action(() => {});
  const derived = computed(() => save());
  expect(derived).toThrow("action was called while a computed");
});

test("`$action` without the compiler throws, and is typed as the action it compiles to", () => {
  const types = () => {
    const toggle = $action(async (todo: Todo) => {
      todo.done = !todo.done;
    });
    expectTypeOf(toggle).toEqualTypeOf<Action<[todo: Todo], void>>();
    expectTypeOf(toggle.pending).toEqualTypeOf<number>();
  };
  expect(types).toThrow("requires the reze compiler");
});

test.each([
  ["another action in flight", (list: string[]) => void speculate(() => list.push("b"))(gate().promise).catch(() => {})],
  ["a plain write", (list: string[]) => void list.push("b")],
])("undoing a length change of an array also changed by %s warns in development", async (_, interfere) => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const state = store({ list: [] as string[] });
  const wait = gate();
  const result = speculate(() => state.list.push("a"))(wait.promise).catch(() => {});
  interfere(state.list);
  wait.reject();
  await result;
  expect(warn).toHaveBeenCalledOnce();
  warn.mockClear();
  const alone = gate();
  const failing = speculate(() => state.list.push("c"))(alone.promise).catch(() => {});
  alone.reject();
  await failing;
  expect(warn).not.toHaveBeenCalled();
  warn.mockRestore();
});
