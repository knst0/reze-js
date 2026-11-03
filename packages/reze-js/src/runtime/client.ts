type Entry = unknown[];

class Effect {
  readonly sources = new Set<Signal<unknown>>();
  cleanups: (() => void)[] = [];

  constructor(readonly fn: () => void) {}
}

function run(effect: Effect): void {
  for (const cleanup of effect.cleanups.splice(0)) cleanup();
  for (const source of effect.sources) source.subscribers.delete(effect);
  effect.sources.clear();
  const previous = current;
  current = effect;
  try {
    effect.fn();
  } finally {
    current = previous;
  }
}

let current: Effect | null = null;
const pending = new Set<Effect>();

function flush(): void {
  for (const effect of pending) {
    pending.delete(effect);
    run(effect);
  }
}

export class Signal<T> {
  readonly subscribers = new Set<Effect>();

  constructor(protected value: T) {}

  get v(): T {
    if (current) {
      this.subscribers.add(current);
      current.sources.add(this as Signal<unknown>);
    }
    return this.value;
  }

  set v(next: T) {
    if (Object.is(next, this.value)) return;
    this.value = next;
    this.notify();
  }

  notify(): void {
    if (pending.size === 0) queueMicrotask(flush);
    for (const subscriber of this.subscribers) pending.add(subscriber);
  }
}

export const $sig = <T>(value: T): Signal<T> => new Signal(value);

class Store<T> extends Signal<T> {
  readonly #proxies = new WeakMap<object, object>();

  override get v(): T {
    return this.#wrap(super.v);
  }

  override set v(next: T) {
    super.v = next;
  }

  #wrap<V>(value: V): V {
    if (typeof value !== "object" || value === null) return value;
    let proxy = this.#proxies.get(value);
    if (!proxy) {
      proxy = new Proxy(value, {
        get: (target, key) => this.#wrap(Reflect.get(target, key)),
        set: (target, key, next) => {
          const changed = !Object.is(Reflect.get(target, key), next);
          Reflect.set(target, key, next);
          if (changed || key === "length") this.notify();
          return true;
        },
        deleteProperty: (target, key) => {
          Reflect.deleteProperty(target, key);
          this.notify();
          return true;
        },
      });
      this.#proxies.set(value, proxy);
    }
    return proxy as V;
  }
}

export const $store = <T>(value: T): Signal<T> => new Store(value);

export function $fx(fn: () => void): void {
  run(new Effect(fn));
}

export function $comp<T>(fn: () => T): Signal<T> {
  const signal = new Signal(undefined as T);
  $fx(() => (signal.v = fn()));
  return signal;
}

export function $cleanup(fn: () => void): void {
  current?.cleanups.push(fn);
}

export function $untrack<T>(fn: () => T): T {
  const previous = current;
  current = null;
  try {
    return fn();
  } finally {
    current = previous;
  }
}

export function $str(value: unknown): string {
  return value == null || typeof value === "boolean" ? "" : String(value as string);
}

export function $attr(element: Element, name: string, value: unknown): void {
  if (typeof value === "boolean" && name.startsWith("aria-")) element.setAttribute(name, String(value));
  else if (value == null || value === false) element.removeAttribute(name);
  else element.setAttribute(name, value === true ? "" : String(value as string));
}

export function $prop(element: Element, name: string, value: unknown): void {
  (element as unknown as Record<string, unknown>)[name] = value;
}

/**
 * Splits the text after `prev` (or at the start of `parent`), past `skip` `<!>` separators, into
 * the static lengths and binding values of `items`, and returns the text nodes of the bindings.
 */
export function $text(parent: Node, prev: Node | null, skip: number, items: (number | string)[]): Text[] {
  let node = prev ? prev.nextSibling : parent.firstChild;
  for (let seen = 0; seen < skip && node; node = node.nextSibling) {
    if (node.nodeType === 8) seen++;
  }
  const out: Text[] = [];
  for (const item of items) {
    const length = typeof item === "number" ? item : item.length;
    if (length === 0) {
      if (typeof item === "string") out.push(parent.insertBefore(document.createTextNode(""), node));
      continue;
    }
    const text = node as Text;
    node = text.length > length ? text.splitText(length) : text.nextSibling;
    if (typeof item === "string") out.push(text);
  }
  return out;
}

const key = (scope: number, mark: number) => `${scope.toString(36)}.${mark.toString(36)}`;

export function $q(scope: number, mark: number): NodeListOf<Element> {
  return document.querySelectorAll(`[data-rz="${key(scope, mark)}"]`);
}

export function $cm(scope: number, mark: number): Comment[] {
  const data = key(scope, mark);
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_COMMENT);
  const out: Comment[] = [];
  while (walker.nextNode()) {
    if ((walker.currentNode as Comment).data === data) out.push(walker.currentNode as Comment);
  }
  return out;
}

export function $rows(entries: Entry[], parent: number, kind: number): [number, Entry][] {
  const out: [number, Entry][] = [];
  entries.forEach((entry, index) => {
    if (entry[0] === ~parent && entry[1] === kind) out.push([index, entry]);
  });
  return out;
}
