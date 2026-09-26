import { untrack } from "./owner";
import { SignalNode } from "./signal";

type Key = string | symbol;

interface TrackedObject {
  properties: Map<Key, SignalNode>;
  keys: SignalNode | undefined;
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
export function store<T extends object>(init: T): [state: T, setState: (fn: (draft: T) => void) => void] {
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
  return typeof value === "object" && value !== null ? ((targetOfProxy.get(value) as T | undefined) ?? value) : value;
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
  (entry.keys ??= new SignalNode<unknown>(undefined, false)).read();
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
      property = new SignalNode<unknown>(toTarget(desc?.value), Object.is);
      properties.set(key, property);
    }
    const value = toTarget(property.read());
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
    const property = entry.properties.get(key);
    if (property !== undefined) property.write((target as Record<Key, unknown>)[key]);
    let keysChanged = !hadKey;
    if (Array.isArray(target) && target.length !== prevLength) {
      const length = entry.properties.get("length");
      if (length !== undefined) length.write(target.length);
      for (let i = target.length; i < prevLength; i++) {
        const removed = entry.properties.get(String(i));
        if (removed !== undefined) removed.write(undefined);
        keysChanged = true;
      }
    }
    if (keysChanged && entry.keys !== undefined) entry.keys.write(undefined);
    return true;
  }

  deleteProperty(target: object, key: Key): boolean {
    const hadKey = Object.hasOwn(target, key);
    if (!Reflect.deleteProperty(target, key)) return false;
    const entry = tracked.get(target);
    if (hadKey && entry !== undefined) {
      const property = entry.properties.get(key);
      if (property !== undefined) property.write(undefined);
      if (entry.keys !== undefined) entry.keys.write(undefined);
    }
    return true;
  }
}
