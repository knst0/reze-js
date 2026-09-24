import { untrack } from "./owner";
import { signal, type Getter, type Setter } from "./signal";

type Key = string | symbol;
type Signal = [Getter<unknown>, Setter<unknown>];

interface TrackedObject {
  properties: Map<Key, Signal>;
  keys: Signal | undefined;
}

const tracked = new WeakMap<object, TrackedObject>();
const stateProxies = new WeakMap<object, object>();
const targetOfProxy = new WeakMap<object, object>();

/**
 * A deep reactive object: `state` reads as if every own property of every object and array in
 * the tree were a `signal` (plus one signal per key set), and throws `TypeError` on writes.
 * `setState(fn)` runs `fn(draft)` untracked; writes through `draft` notify immediately, and
 * `draft` proxies throw `TypeError` once `fn` returns. `init` is adopted, not copied.
 */
export function store<T extends object>(
  init: T,
): [state: T, setState: (fn: (draft: T) => void) => void] {
  const target = toTarget(init) as T;
  return [
    stateProxy(target),
    (fn) => {
      const drafts = new DraftHandler();
      try {
        untrack(() => fn(drafts.proxy(target)));
      } finally {
        drafts.revoke();
      }
    },
  ];
}

function toTarget<T>(value: T): T {
  return typeof value === "object" && value !== null
    ? ((targetOfProxy.get(value) as T | undefined) ?? value)
    : value;
}

function isWrappable(value: unknown): value is object {
  if (typeof value !== "object" || value === null) return false;
  const proto = Object.getPrototypeOf(value);
  return proto === Object.prototype || proto === null || Array.isArray(value);
}

function trackedObject(target: object): TrackedObject {
  let entry = tracked.get(target);
  if (entry === undefined) {
    entry = { properties: new Map(), keys: undefined };
    tracked.set(target, entry);
  }
  return entry;
}

function readKeys(target: object): void {
  const entry = trackedObject(target);
  (entry.keys ??= signal<unknown>(undefined, { equals: false }))[0]();
}

function isFrozenData(desc: PropertyDescriptor): boolean {
  return !desc.writable && !desc.configurable;
}

function stateProxy<T extends object>(target: T): T {
  let proxy = stateProxies.get(target);
  if (proxy === undefined) {
    proxy = new Proxy(target, stateHandler);
    stateProxies.set(target, proxy);
    targetOfProxy.set(proxy, target);
  }
  return proxy as T;
}

function rejectWrite(): never {
  throw new TypeError("store state is read-only; write through setState");
}

const stateHandler: ProxyHandler<object> = {
  get(target, key, receiver) {
    const properties = trackedObject(target).properties;
    let property = properties.get(key);
    if (property === undefined) {
      const desc = Reflect.getOwnPropertyDescriptor(target, key);
      if (desc ? !("value" in desc) || isFrozenData(desc) : key in target) {
        return Reflect.get(target, key, receiver);
      }
      property = signal<unknown>(toTarget(desc?.value));
      properties.set(key, property);
    }
    const value = toTarget(property[0]());
    return isWrappable(value) ? stateProxy(value) : value;
  },
  has(target, key) {
    readKeys(target);
    return Reflect.has(target, key);
  },
  ownKeys(target) {
    readKeys(target);
    return Reflect.ownKeys(target);
  },
  set: rejectWrite,
  deleteProperty: rejectWrite,
  defineProperty: rejectWrite,
  setPrototypeOf: rejectWrite,
  preventExtensions: rejectWrite,
};

class DraftHandler implements ProxyHandler<object> {
  private readonly drafts = new Map<object, { proxy: object; revoke: () => void }>();

  proxy<T extends object>(target: T): T {
    let draft = this.drafts.get(target);
    if (draft === undefined) {
      draft = Proxy.revocable(target, this);
      this.drafts.set(target, draft);
      targetOfProxy.set(draft.proxy, target);
    }
    return draft.proxy as T;
  }

  revoke(): void {
    for (const draft of this.drafts.values()) draft.revoke();
    this.drafts.clear();
  }

  get(target: object, key: Key, receiver: unknown): unknown {
    const desc = Reflect.getOwnPropertyDescriptor(target, key);
    if (desc === undefined || !("value" in desc) || isFrozenData(desc)) {
      return Reflect.get(target, key, receiver);
    }
    const value = toTarget(desc.value);
    return isWrappable(value) ? this.proxy(value) : value;
  }

  set(target: object, key: Key, value: unknown, receiver: unknown): boolean {
    return Reflect.set(target, key, toTarget(value), receiver);
  }

  defineProperty(target: object, key: Key, desc: PropertyDescriptor): boolean {
    const hadKey = Object.hasOwn(target, key);
    const prevLength = Array.isArray(target) ? target.length : 0;
    if ("value" in desc) desc.value = toTarget(desc.value);
    if (!Reflect.defineProperty(target, key, desc)) return false;
    const entry = tracked.get(target);
    if (entry === undefined) return true;
    const value = (target as Record<Key, unknown>)[key];
    entry.properties.get(key)?.[1](() => value);
    let keysChanged = !hadKey;
    if (Array.isArray(target) && target.length !== prevLength) {
      entry.properties.get("length")?.[1](target.length);
      for (let i = target.length; i < prevLength; i++) {
        entry.properties.get(String(i))?.[1](undefined);
        keysChanged = true;
      }
    }
    if (keysChanged) entry.keys?.[1](undefined);
    return true;
  }

  deleteProperty(target: object, key: Key): boolean {
    const hadKey = Object.hasOwn(target, key);
    if (!Reflect.deleteProperty(target, key)) return false;
    const entry = tracked.get(target);
    if (hadKey && entry !== undefined) {
      entry.properties.get(key)?.[1](undefined);
      entry.keys?.[1](undefined);
    }
    return true;
  }
}
