type Entry = unknown[];

class Effect {
  readonly _sources = new Set<Signal<unknown>>();
  _cleanups: (() => void)[] = [];

  constructor(readonly _fn: () => void) {}
}

function clear(effect: Effect): void {
  for (const cleanup of effect._cleanups.splice(0)) cleanup();
  for (const source of effect._sources) source._subscribers.delete(effect);
  effect._sources.clear();
}

function run(effect: Effect): void {
  clear(effect);
  const [previous, parent] = [current, owner];
  current = effect;
  owner = effect._cleanups;
  try {
    effect._fn();
  } finally {
    current = previous;
    owner = parent;
  }
}

let current: Effect | null = null;
/** The cleanups that effects and ranges created now belong to. */
let owner: (() => void)[] | null = null;
/** Whether client code is creating nodes rather than adopting the server's. */
let fresh = false;
/** Whether an effect runs for the first time. */
let initial = false;
const pending = new Set<Effect>();

function flush(): void {
  for (const effect of pending) {
    pending.delete(effect);
    run(effect);
  }
}

export class Signal<T> {
  readonly _subscribers = new Set<Effect>();

  constructor(protected _value: T) {}

  get v(): T {
    this._track();
    return this._value;
  }

  _track(): void {
    if (current) {
      this._subscribers.add(current);
      current._sources.add(this as Signal<unknown>);
    }
  }

  set v(next: T) {
    if (Object.is(next, this._value)) return;
    this._value = next;
    this._notify();
  }

  _notify(): void {
    if (pending.size === 0) queueMicrotask(flush);
    for (const subscriber of this._subscribers) pending.add(subscriber);
  }
}

export const $sig = <T>(value: T): Signal<T> => new Signal(value);

/** The field of a store object that reads of its keys subscribe to. */
const keys = Symbol();

class Store<T> extends Signal<T> {
  readonly _proxies = new WeakMap<object, object>();
  readonly _fields = new WeakMap<object, Map<PropertyKey, Signal<undefined>>>();

  override get v(): T {
    return this._wrap(super.v);
  }

  override set v(next: T) {
    super.v = next;
  }

  _field(target: object, key: PropertyKey): Signal<undefined> {
    let fields = this._fields.get(target);
    if (!fields) this._fields.set(target, (fields = new Map()));
    let field = fields.get(key);
    if (!field) fields.set(key, (field = new Signal(undefined)));
    return field;
  }

  /** Notifies the readers of `key`, and of the keys a write added, removed, or cut off by `length`. */
  _write(target: object, key: PropertyKey, write: () => void): void {
    const [had, previous, length] = [Object.hasOwn(target, key), Reflect.get(target, key), (target as unknown[]).length];
    write();
    if (!Object.is(previous, Reflect.get(target, key))) this._field(target, key)._notify();
    if (had !== Object.hasOwn(target, key)) this._field(target, keys)._notify();
    if (Array.isArray(target) && length !== target.length) {
      for (const [field, signal] of this._fields.get(target) ?? [])
        if (field === "length" || +String(field) >= target.length) signal._notify();
    }
  }

  _wrap<V>(value: V): V {
    if (typeof value !== "object" || value === null) return value;
    let proxy = this._proxies.get(value);
    if (!proxy) {
      proxy = new Proxy(value, {
        get: (target, key) => {
          this._field(target, key)._track();
          return this._wrap(Reflect.get(target, key));
        },
        has: (target, key) => {
          this._field(target, key)._track();
          return Reflect.has(target, key);
        },
        ownKeys: (target) => {
          this._field(target, keys)._track();
          return Reflect.ownKeys(target);
        },
        set: (target, key, next) => {
          this._write(target, key, () => Reflect.set(target, key, next));
          return true;
        },
        deleteProperty: (target, key) => {
          this._write(target, key, () => Reflect.deleteProperty(target, key));
          return true;
        },
      });
      this._proxies.set(value, proxy);
    }
    return proxy as V;
  }
}

export const $store = <T>(value: T): Signal<T> => new Store(value);

export function $fx(fn: () => void): void {
  const effect = new Effect(fn);
  owner?.push(() => {
    clear(effect);
    pending.delete(effect);
  });
  const outer = initial;
  initial = true;
  try {
    run(effect);
  } finally {
    initial = outer;
  }
}

/** Runs `fn` untracked, as the owner of what it creates, and returns the disposal. */
function root(create: boolean, fn: () => void): () => void {
  const cleanups: (() => void)[] = [];
  const [previous, parent, adopting] = [current, owner, fresh];
  [current, owner, fresh] = [null, cleanups, create];
  try {
    fn();
  } finally {
    [current, owner, fresh] = [previous, parent, adopting];
  }
  return () => {
    for (const cleanup of cleanups.splice(0)) cleanup();
  };
}

export function $comp<T>(fn: () => T): Signal<T> {
  const signal = new Signal(undefined as T);
  $fx(() => (signal.v = fn()));
  return signal;
}

export { buildPaths as $path } from "./paths";
export { MutationInputError } from "./errors";

/** The browser side of a mutation: a call, or `.call`, posts through the navigation module. */
export function $call(id: string): ((input?: unknown) => Promise<unknown>) & { id: string; call(input?: unknown): Promise<unknown> } {
  const call = (input?: unknown) => nav().then((module) => module.call(id, input));
  return Object.assign(call, { id, call });
}

export const $loc = (): Location => location;

export const $search = (): URLSearchParams => new URLSearchParams(location.search);

export const $link = (href: unknown): { active: boolean } => ({
  active: typeof href === "string" && href[0] === "/" && href[1] !== "/" && href.split(/[?#]/)[0] === location.pathname,
});

export function $cleanup(fn: () => void): void {
  owner?.push(fn);
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

const defaults: Record<string, string> = { value: "defaultValue", checked: "defaultChecked", selected: "defaultSelected" };

/**
 * Sets a property. The first run of a binding over the server's nodes leaves a field the user
 * changed, so input typed before the chunk loaded survives the binding.
 */
export function $prop(element: Element, name: string, value: unknown): void {
  const target = element as unknown as Record<string, unknown>;
  if (initial && !fresh && name in defaults && target[name] !== target[defaults[name]!]) return;
  target[name] = value;
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
    const length = typeof item === "number" ? item : fresh ? 0 : item.length;
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

/** The elements of a mark, in the document or under the nodes `root`. */
export function $q(scope: number, mark: number, root?: Node[]): Element[] {
  const selector = `[data-rz="${key(scope, mark)}"]`;
  if (!root) return [...document.querySelectorAll(selector)];
  const out: Element[] = [];
  for (const node of root) {
    if (!(node instanceof Element)) continue;
    if (node.matches(selector)) out.push(node);
    out.push(...node.querySelectorAll(selector));
  }
  return out;
}

function comments(data: string, root: Node[] = [document.body]): Comment[] {
  const out: Comment[] = [];
  for (const node of root) {
    const walker = document.createTreeWalker(node, NodeFilter.SHOW_COMMENT);
    for (let current: Node | null = walker.currentNode; current; current = walker.nextNode()) {
      if (current.nodeType === 8 && (current as Comment).data === data) out.push(current as Comment);
    }
  }
  return out;
}

export const $cm = (scope: number, mark: number, root?: Node[]): Comment[] => comments(key(scope, mark), root);

/** The start markers of a range, `<!--[scope.mark-->`. */
export const $rg = (scope: number, mark: number, root?: Node[]): Comment[] => comments(`[${key(scope, mark)}`, root);

export function $rows(entries: Entry[], parent: number, kind: number): [number, Entry][] {
  const out: [number, Entry][] = [];
  entries.forEach((entry, index) => {
    if (entry[0] === ~parent && entry[1] === kind) out.push([index, entry]);
  });
  return out;
}

/** `aria-current` and `data-active` of an anchor whose `href` is the current path. */
export function $ac(element: Element, href: unknown): void {
  const active = $link(href).active;
  $attr(element, "aria-current", active && "page");
  $attr(element, "data-active", active);
}

/** Creates the nodes of `html` before `at`; `svg` parses them as SVG content. */
export function $h(html: string, at: Node, svg?: number): Node[] {
  const template = document.createElement("template");
  template.innerHTML = svg ? `<svg>${html}</svg>` : html;
  const parent = svg ? template.content.firstChild! : template.content;
  const nodes = [...parent.childNodes];
  for (const node of nodes) at.parentNode!.insertBefore(node, at);
  return nodes;
}

/** The `<!--]-->` that closes the range `start` opens. */
function close(start: Node): Node {
  let depth = 0;
  for (let node = start.nextSibling!; ; node = node.nextSibling!) {
    if (node.nodeType !== 8) continue;
    const data = (node as Comment).data;
    if (data[0] === "[") depth++;
    else if (data === "]" && !depth--) return node;
  }
}

function between(start: Node, end: Node): Node[] {
  const out: Node[] = [];
  for (let node = start.nextSibling!; node !== end; node = node.nextSibling!) out.push(node);
  return out;
}

const remove = (nodes: Node[]) => {
  for (const node of nodes) node.parentNode?.removeChild(node);
};

type Render = (n: Node[] | null, at: Node, ...args: unknown[]) => Node[] | null;

/**
 * A `<Show>` or `<Switch>`: renders the branch of the first true condition, or the fallback at
 * `conditions.length`. A branch whose bit is set in `keyed` renders again when its value changes.
 */
export function $range(start: Comment, conditions: (() => unknown)[], keyed: number, render: Render): void {
  const end = close(start);
  let nodes: Node[] | null = fresh ? null : between(start, end);
  let branch = -1;
  let value: unknown;
  let dispose = () => {};
  owner?.push(() => dispose());
  $fx(() => {
    let next = 0;
    let current: unknown;
    while (next < conditions.length && !(current = conditions[next]!())) next++;
    if (next === branch && !((keyed >> next) & 1 && !Object.is(current, value))) return;
    [branch, value] = [next, current];
    dispose();
    const adopted = nodes;
    if (!adopted) remove(between(start, end));
    nodes = null;
    dispose = root(!adopted, () => {
      render(adopted, end, branch, value);
    });
  });
}

interface Row {
  item: unknown;
  first: Node | undefined;
  index: Signal<number>;
  dispose: () => void;
}

/**
 * A `<For>` or `<Repeat>` (a number source): one row per item, keyed by identity. `separated`
 * rows open with `<!--,-->`; others are one element. `scope` adds a state entry per row, for
 * handlers: `[entries, parent scope, kind]`.
 */
export function $list(
  start: Comment,
  each: () => unknown,
  render: Render,
  separated: number,
  fallback: Render | 0,
  scope?: [Entry[], number, number],
): void {
  const end = close(start);
  let adopt: Node[] | null = fresh ? null : between(start, end);
  let rows: Row[] = [];
  let empty: { dispose: () => void } | null = null;
  owner?.push(() => {
    for (const row of rows) row.dispose();
    empty?.dispose();
  });
  const extent = (row: Row): Node[] => {
    if (!row.first) return [];
    if (!separated) return [row.first];
    const out = [row.first];
    for (let node = row.first.nextSibling!, depth = 0; node !== end; node = node.nextSibling!) {
      const data = node.nodeType === 8 ? (node as Comment).data : undefined;
      if (!depth && data === ",") break;
      if (data?.[0] === "[") depth++;
      else if (data === "]") depth--;
      out.push(node);
    }
    return out;
  };
  const make = (item: unknown, index: number, nodes: Node[] | null): Row => {
    const row: Row = { item, first: undefined, index: $sig(index), dispose: () => {} };
    let entry: number | undefined;
    if (scope) {
      const [entries, parent, kind] = scope;
      entry = nodes ? entries.findIndex((e) => e[0] === ~parent && e[1] === kind && e.length < 3) : -1;
      if (entry < 0) {
        const { u: modules } = (window as unknown as { rz: Rz }).rz;
        modules[(entry = entries.push([~parent, kind]) - 1)] = modules[parent]!;
      }
      entries[entry]!.push(item, row.index);
    }
    row.dispose = root(!nodes, () => {
      row.first = render(nodes, end, entry, item, row.index)?.[0];
    });
    return row;
  };
  $fx(() => {
    const source = each();
    const items =
      typeof source === "number"
        ? Array.from({ length: source > 0 ? source : 0 }, (_, index) => index)
        : [...((source as Iterable<unknown> | null | undefined) ?? [])];
    $untrack(() => {
      if (adopt) {
        const nodes = adopt;
        adopt = null;
        if (!items.length) {
          if (fallback) empty = { dispose: root(false, () => fallback(nodes, end)) };
          else remove(nodes);
          return;
        }
        const groups = split(nodes, separated);
        if (groups.length === items.length) {
          rows = items.map((item, index) => make(item, index, groups[index]!));
          return;
        }
        remove(nodes);
      }
      if (items.length && empty) {
        empty.dispose();
        remove(between(start, end));
        empty = null;
      }
      const old = new Map<unknown, Row[]>();
      for (const row of rows) {
        const same = old.get(row.item);
        if (same) same.push(row);
        else old.set(row.item, [row]);
      }
      const next: (Row | undefined)[] = items.map((item) => old.get(item)?.shift());
      for (const same of old.values()) {
        for (const row of same) {
          row.dispose();
          remove(extent(row));
        }
      }
      rows = next.map((row, index) => {
        if (!row) return make(items[index], index, null);
        row.index.v = index;
        return row;
      });
      const extents = rows.map(extent);
      let cursor = start.nextSibling;
      for (const nodes of extents) {
        if (nodes[0] === cursor) cursor = nodes[nodes.length - 1]!.nextSibling;
        else for (const node of nodes) end.parentNode!.insertBefore(node, cursor);
      }
      if (!rows.length && fallback && !empty) empty = { dispose: root(true, () => fallback(null, end)) };
    });
  });
}

/** The server's rows between a list's markers: elements, or groups that open with `<!--,-->`. */
function split(nodes: Node[], separated: number): Node[][] {
  const out: Node[][] = [];
  let depth = 0;
  for (const node of nodes) {
    const data = node.nodeType === 8 ? (node as Comment).data : undefined;
    if (separated ? !depth && data === "," : node.nodeType === 1) out.push([node]);
    else out[out.length - 1]?.push(node);
    if (data?.[0] === "[") depth++;
    else if (data === "]") depth--;
  }
  return out;
}

type Handlers = Record<number, Record<string, (row: Entry) => (event: Event) => unknown>>;
type Factory = (entry: Entry, index: number, entries: Entry[], instance: (index: number) => Handlers) => Handlers;

/** A loaded client chunk: the factories by id, those that start without an event, and `$run`. */
export interface Chunk {
  default: Record<number, Factory>;
  g: number[];
  r: typeof $run;
}

/**
 * The dispatcher's state: the entries, the chunk URL of each entry, the loaded chunks by URL, and
 * the factories that start without an event.
 */
export interface Rz {
  e: Entry[];
  /** The manifest index of each entry's chunk. */
  u: number[];
  c: Record<number, Chunk>;
  s: number[];
}

const instances: Handlers[] = [];
/** The navigation module, through the loader of the links module. */
const nav = () => (window as unknown as { rzNav(): Promise<typeof import("virtual:rz-nav")> }).rzNav();

/**
 * Runs the handlers of `sites` for `event`, innermost first, or without an event, instantiates
 * the entries whose chunks are loaded and that start without an event.
 */
export function $run(rz: Rz, sites?: Element[], event?: Event): void {
  const { e: entries, u: modules, c: chunks } = rz;
  const instance = (index: number): Handlers =>
    (instances[index] ??= chunks[modules[index]!]!.default[entries[index]![0] as number]!(entries[index]!, index, entries, instance));
  const fault = (node: Element, error: unknown) =>
    void nav().then(
      (module) => module.fault(node, error),
      () => reportError(error),
    );
  if (!sites || !event) {
    entries.forEach((entry, index) => {
      const chunk = chunks[modules[index]!];
      if (chunk && !instances[index] && (rz.s.includes(entry[0] as number) || chunk.g.includes(entry[0] as number))) instance(index);
    });
    return;
  }
  for (const node of sites) {
    const [scope, mark] = node
      .getAttribute("data-rz")!
      .split(".")
      .map((part) => parseInt(part, 36)) as [number, number];
    let index = scope;
    while ((entries[index]![0] as number) < 0) index = ~(entries[index]![0] as number);
    const handler = instance(index)[mark]?.[event.type];
    if (!handler) continue;
    Object.defineProperty(event, "currentTarget", { value: node, configurable: true });
    try {
      const result = handler(entries[scope]!)(event);
      if (result instanceof Promise) result.catch((error: unknown) => fault(node, error));
    } catch (error) {
      fault(node, error);
    }
    delete (event as { currentTarget?: unknown }).currentTarget;
    if (event.cancelBubble) break;
  }
}
