import { isPureRun } from "./context";
import { profileCreated } from "./profile";
import { SignalNode } from "./signal";
import { type Key, setWriteHook, writeKey } from "./store";

/**
 * One call of an action. Store writes made while it is current are speculative: visible at once,
 * kept when the call succeeds, undone when it fails. The compiler threads it through `$action`
 * bodies; hand-written actions call `resume` after every `await`.
 */
export interface Run {
  /** Makes this run current, remembering the one it replaces; returns `value`. */
  resume<T>(value?: T): T;
  /** Makes the run `resume` replaced current again; returns `value`, the operand of the next `await`. */
  suspend<T>(value: T): T;
  /** `suspend` at the end of the body. */
  end(): void;
}

export interface Action<Args extends unknown[], R> {
  /** Starts a run; the promise rejects with what the run threw after its store writes are undone. */
  (...args: Args): Promise<R>;
  /** Runs in flight; tracked. */
  readonly pending: number;
  /** What the latest failed run threw; cleared when the next run starts; tracked. */
  readonly error: unknown;
}

export interface ActionOptions {
  /** The name profiling attribution shows for `pending` and `error`; ignored in production builds. */
  name?: string;
}

interface Layer {
  run: ActionRun;
  value: unknown;
}

/** The speculative writes of every live run to one key; the visible value is the top layer's. */
interface KeyRecord {
  target: object;
  key: Key;
  /** The latest non-speculative value, `ABSENT` for a missing key. */
  base: unknown;
  /** Whether a write outside any run landed in `base` while layers hid it. */
  isBaseDirty: boolean;
  /** Oldest first; at most one per run. */
  layers: Layer[];
}

const journal = new WeakMap<object, Map<Key, KeyRecord>>();
let current: ActionRun | undefined;

class ActionRun implements Run {
  prev: ActionRun | undefined = undefined;
  isCurrent = false;
  /** In order of the first write. */
  records: KeyRecord[] = [];

  resume<T>(value?: T): T {
    if (!this.isCurrent) {
      this.prev = current;
      // oxlint-disable-next-line no-this-alias -- the current run is module state, restored by `suspend`
      current = this;
      this.isCurrent = true;
    }
    return value as T;
  }

  suspend<T>(value: T): T {
    if (this.isCurrent) {
      current = this.prev;
      this.prev = undefined;
      this.isCurrent = false;
    }
    return value;
  }

  end(): void {
    this.suspend(undefined);
  }
}

function layerOf(layers: Layer[], run: ActionRun): number {
  for (let i = layers.length - 1; i >= 0; i--) {
    if (layers[i]!.run === run) return i;
  }
  return -1;
}

function drop(record: KeyRecord): void {
  const records = journal.get(record.target)!;
  records.delete(record.key);
  if (records.size === 0) journal.delete(record.target);
}

function journalWrite(target: object, key: Key, prev: unknown, next: unknown): boolean {
  const run = current;
  let records = journal.get(target);
  let record = records?.get(key);
  if (run === undefined) {
    if (record === undefined) return false;
    record.base = next;
    record.isBaseDirty = true;
    return true;
  }
  if (record === undefined) {
    record = { target, key, base: prev, isBaseDirty: false, layers: [] };
    if (records === undefined) journal.set(target, (records = new Map()));
    records.set(key, record);
  }
  const layers = record.layers;
  const at = layerOf(layers, run);
  if (at === layers.length - 1 && at !== -1) {
    layers[at]!.value = next;
    return false;
  }
  if (at === -1) run.records.push(record);
  else layers.splice(at, 1);
  layers.push({ run, value: next });
  return false;
}

/** The run's writes become the base; older layers of the same keys are superseded by them. */
function commit(run: ActionRun): void {
  for (const record of run.records) {
    const layers = record.layers;
    const at = layerOf(layers, run);
    if (at === -1) continue;
    if (!record.isBaseDirty) record.base = layers[at]!.value;
    layers.splice(0, at + 1);
    if (layers.length === 0) {
      drop(record);
      if (record.isBaseDirty) writeKey(record.target, record.key, record.base);
    }
  }
}

/** Removes the run's layers newest key first; a removed top layer shows the next one, or the base. */
function rollback(run: ActionRun): void {
  const records = run.records;
  for (let i = records.length - 1; i >= 0; i--) {
    const record = records[i]!;
    const layers = record.layers;
    const at = layerOf(layers, run);
    if (at === -1) continue;
    layers.splice(at, 1);
    const isTop = at === layers.length;
    if (
      process.env.NODE_ENV !== "production" &&
      record.key === "length" &&
      Array.isArray(record.target) &&
      (record.isBaseDirty || !isTop)
    ) {
      console.warn(
        "[rezejs] An action that changed an array's length failed while the array was also changed outside it " +
          "(by a plain write or another action in flight); each index was restored on its own, so the array may mix " +
          "both versions or keep holes.",
      );
    }
    if (!isTop) continue;
    if (layers.length !== 0) {
      writeKey(record.target, record.key, layers[at - 1]!.value);
      continue;
    }
    drop(record);
    writeKey(record.target, record.key, record.base);
  }
}

function isThenable(value: unknown): value is PromiseLike<unknown> {
  return (
    (typeof value === "object" || typeof value === "function") &&
    value !== null &&
    typeof (value as PromiseLike<unknown>).then === "function"
  );
}

class ActionState<Args extends unknown[], R> {
  fn: (run: Run, ...args: Args) => R | PromiseLike<R>;
  name: string | undefined;
  running = 0;
  failure: unknown = undefined;
  runningNode: SignalNode<number> | undefined = undefined;
  failureNode: SignalNode<unknown> | undefined = undefined;

  constructor(fn: (run: Run, ...args: Args) => R | PromiseLike<R>, name: string | undefined) {
    this.fn = fn;
    this.name = name;
  }

  node<T>(value: T, field: string): SignalNode<T> {
    const node = new SignalNode(value, Object.is);
    if (process.env.NODE_ENV !== "production") {
      profileCreated(node, "signal", this.name === undefined ? undefined : `${this.name}.${field}`);
    }
    return node;
  }

  pending(): number {
    return (this.runningNode ??= this.node(this.running, "pending")).read();
  }

  error(): unknown {
    return (this.failureNode ??= this.node(this.failure, "error")).read();
  }

  setRunning(running: number): void {
    this.running = running;
    this.runningNode?.write(running);
  }

  setFailure(failure: unknown): void {
    this.failure = failure;
    this.failureNode?.write(failure);
  }

  call(args: Args): Promise<R> {
    if (process.env.NODE_ENV !== "production" && isPureRun()) {
      throw new Error("[rezejs] An action was called while a computed or render binding runs; call it from an event handler or effect.");
    }
    setWriteHook(journalWrite);
    this.setFailure(undefined);
    this.setRunning(this.running + 1);
    const run = new ActionRun();
    let result: R | PromiseLike<R>;
    run.resume();
    try {
      result = this.fn(run, ...args);
    } catch (error) {
      this.fail(run, error);
      return Promise.reject(error);
    } finally {
      run.suspend(undefined);
    }
    if (!isThenable(result)) {
      this.succeed(run);
      return Promise.resolve(result);
    }
    return Promise.resolve(result).then(
      (value) => {
        this.succeed(run);
        return value;
      },
      (error: unknown) => {
        this.fail(run, error);
        throw error;
      },
    );
  }

  succeed(run: ActionRun): void {
    commit(run);
    this.setRunning(this.running - 1);
  }

  fail(run: ActionRun, error: unknown): void {
    rollback(run);
    this.setFailure(error);
    this.setRunning(this.running - 1);
  }
}

/**
 * Wraps `fn` into an action: each call runs `fn(run, ...args)` with `run` current, so store writes
 * it makes are speculative until the result settles. `$action` compiles to this.
 */
export function action<Args extends unknown[], R>(
  fn: (run: Run, ...args: Args) => R | PromiseLike<R>,
  options?: ActionOptions,
): Action<Args, R> {
  const state = new ActionState(fn, options?.name);
  const act = (...args: Args): Promise<R> => state.call(args);
  Object.defineProperties(act, {
    pending: { get: () => state.pending() },
    error: { get: () => state.error() },
  });
  return act as Action<Args, R>;
}
