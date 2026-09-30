import { isPureRun } from "./context";
import { SignalNode } from "./signal";

export type Key = string | symbol;

interface TrackedObject {
  properties: Map<Key, SignalNode>;
  keys: SignalNode | undefined;
}

/** The value of a key an object does not have. */
export const ABSENT: unique symbol = Symbol("absent");

/**
 * Sees every write of a store data property before it lands; `prev` and `next` are `ABSENT` for a
 * missing key. Returns `true` when it took over the write, which then must not be applied.
 */
export type WriteHook = (target: object, key: Key, prev: unknown, next: unknown) => boolean;

let writeHook: WriteHook | undefined;

export function setWriteHook(hook: WriteHook): void {
  writeHook = hook;
}

const tracked = new WeakMap<object, TrackedObject>();
const mutableProxies = new WeakMap<object, object>();
const readonlyProxies = new WeakMap<object, object>();
const targetOfProxy = new WeakMap<object, object>();

/**
 * A deep reactive object: reads track every own property of every plain object and array in the
 * tree as if it were a `signal` (plus one signal per key set), and writes (assignment, `delete`,
 * array methods) change the object in place and notify immediately. `init` is adopted, not copied.
 */
export function store<T extends object>(init: T): T {
  return proxyOf(toTarget(init), mutableProxies, mutableHandler);
}

/** A view of `state` that reads the same signals and throws `TypeError` on every write. */
export function readonly<T extends object>(state: T): T {
  return proxyOf(toTarget(state), readonlyProxies, readonlyHandler);
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

function proxyOf<T extends object>(target: T, proxies: WeakMap<object, object>, handler: ProxyHandler<object>): T {
  let proxy = proxies.get(target);
  if (proxy === undefined) {
    proxy = new Proxy(target, handler);
    proxies.set(target, proxy);
    targetOfProxy.set(proxy, target);
  }
  return proxy as T;
}

function read(target: object, key: Key, receiver: unknown, proxies: WeakMap<object, object>, handler: ProxyHandler<object>): unknown {
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
  const value = property.read();
  return isWrappable(value) ? proxyOf(value, proxies, handler) : value;
}

function ownValue(target: object, key: Key): unknown {
  const desc = Reflect.getOwnPropertyDescriptor(target, key);
  return desc === undefined ? ABSENT : desc.value;
}

const PURE_WRITE = "store written while a computed or render binding runs; derive the value instead, or write from an event or effect";

/** Writes `value` to `key` of the raw object `target` and notifies its readers, bypassing the write hook. */
export function writeKey(target: object, key: Key, value: unknown): void {
  if (value === ABSENT) {
    deleteKey(target, key);
  } else if (Object.hasOwn(target, key)) {
    defineKey(target, key, { value });
  } else {
    defineKey(target, key, { value, writable: true, enumerable: true, configurable: true });
  }
}

function defineKey(target: object, key: Key, desc: PropertyDescriptor): boolean {
  const hadKey = Object.hasOwn(target, key);
  const prevLength = Array.isArray(target) ? target.length : 0;
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

function deleteKey(target: object, key: Key): boolean {
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

/**
 * Hands the hook what a write to an array changes besides `key`: the indices a shorter `length`
 * removes, or the `length` an index past the end grows, so undoing each key undoes the write.
 */
function hookArrayLength(hook: WriteHook, target: unknown[], key: Key, value: unknown): void {
  if (key === "length") {
    if (typeof value !== "number") return;
    for (let i = value; i < target.length; i++) {
      if (Object.hasOwn(target, i)) hook(target, String(i), target[i], ABSENT);
    }
  } else if (typeof key === "string") {
    const index = Number(key);
    if (index >= target.length && String(index >>> 0) === key) hook(target, "length", target.length, index + 1);
  }
}

function rejectWrite(): never {
  throw new TypeError("readonly store state cannot be written");
}

function rejectShapeChange(): never {
  throw new TypeError("store state keeps its prototype and stays extensible");
}

function hasKey(target: object, key: Key): boolean {
  readKeys(target);
  return Reflect.has(target, key);
}

function ownKeys(target: object): Key[] {
  readKeys(target);
  return Reflect.ownKeys(target);
}

const mutableHandler: ProxyHandler<object> = {
  get(target, key, receiver) {
    return read(target, key, receiver, mutableProxies, mutableHandler);
  },
  has: hasKey,
  ownKeys,
  defineProperty(target, key, desc) {
    if (process.env.NODE_ENV !== "production" && isPureRun()) throw new TypeError(PURE_WRITE);
    if (!("value" in desc)) return defineKey(target, key, desc);
    const value = (desc.value = toTarget(desc.value));
    const hook = writeHook;
    if (hook !== undefined) {
      if (hook(target, key, ownValue(target, key), value)) return true;
      if (Array.isArray(target)) hookArrayLength(hook, target, key, value);
    }
    return defineKey(target, key, desc);
  },
  deleteProperty(target, key) {
    if (process.env.NODE_ENV !== "production" && isPureRun()) throw new TypeError(PURE_WRITE);
    if (writeHook !== undefined && Object.hasOwn(target, key) && writeHook(target, key, ownValue(target, key), ABSENT)) return true;
    return deleteKey(target, key);
  },
  setPrototypeOf: rejectShapeChange,
  preventExtensions: rejectShapeChange,
};

const readonlyHandler: ProxyHandler<object> = {
  get(target, key, receiver) {
    return read(target, key, receiver, readonlyProxies, readonlyHandler);
  },
  has: hasKey,
  ownKeys,
  set: rejectWrite,
  deleteProperty: rejectWrite,
  defineProperty: rejectWrite,
  setPrototypeOf: rejectWrite,
  preventExtensions: rejectWrite,
};
