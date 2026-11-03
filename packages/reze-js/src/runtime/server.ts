import { compare, match, type Segment } from "./match";

type Thunk = () => string;

class Markup {
  constructor(readonly content: string | Thunk) {}
}

interface HeadEntry {
  depth: number;
  html: string;
}

/** What `useSubmission` returns: the current request's submission of one mutation. */
export interface Submission<I = Record<string, unknown>, R = unknown> {
  input?: { [K in keyof I]?: string | string[] };
  issues?: { [path: string]: Issue | undefined };
  result?: R;
}

export interface RenderContext {
  depth: number;
  params: Record<string, unknown>;
  url: URL;
  head(key: string | null, html: string): string;
  id(): string;
  /** Adds the state entry of a client component instance and returns its id. */
  scope(component: number, events: string, captures: unknown[], eager: boolean): string;
  /** Adds the state entry of a callback call that client code captures values of. */
  row(parent: string, kind: number, captures: unknown[]): string;
  submission(mutation: Mutation<never, unknown>): Submission;
  /** Renders `children`, or `fallback` with the route error when rendering throws. */
  boundary(children: Thunk, fallback: (error: RouteError) => string): string;
  /** Renders `children`, or `fallback` while they read a deferred field, and streams them later. */
  loading(children: Thunk, fallback: Thunk): string;
}

type Component = (props: Record<string, unknown>, $r: RenderContext) => string;

interface RouteModule {
  default: Component;
  load?: Loader<unknown>;
  guard?: Guard;
  /** Seconds a rendered page stays fresh in a shared cache. */
  revalidate?: number;
  prerender?: boolean;
  /** `"client"` leaves the page's segment empty in a document, and the browser fetches it as a navigation. */
  render?: "client";
  entries?: () => Record<string, string>[] | Promise<Record<string, string>[]>;
}

export interface Route {
  pattern: string;
  segments: Segment[];
  chain: RouteModule[];
  /** Route node of the client chunk, when the page has client code. */
  chunk: number | null;
  /** Whether no loader of the route reads more than its params. */
  static: boolean;
}

/** URLs of the client code. */
export interface Assets {
  dispatcher: string;
  /** The module that moves streamed parts into their `<Loading>` boundaries. */
  stream: string;
  chunk(node: number): string;
}

/** Declares the application's types: `interface Register { context: AppContext }`. */
// oxlint-disable-next-line no-empty-interface
export interface Register {}

type AppContext = Register extends { context: infer C } ? C : unknown;

/** A [Standard Schema v1](https://standardschema.dev) validator. */
export interface StandardSchemaV1<Input = unknown, Output = Input> {
  readonly "~standard": {
    readonly version: 1;
    readonly vendor: string;
    readonly validate: (value: unknown) => StandardResult<Output> | Promise<StandardResult<Output>>;
    readonly types?: { readonly input: Input; readonly output: Output };
  };
}

type StandardResult<Output> = { readonly value: Output; readonly issues?: undefined } | { readonly issues: readonly StandardIssue[] };

interface StandardIssue {
  readonly message: string;
  readonly path?: readonly (PropertyKey | { readonly key: PropertyKey })[];
}

/** A validation issue in the Standard Schema format. */
export interface Issue {
  message: string;
  path: (string | number)[];
}

export interface CookieOptions {
  httpOnly?: boolean;
  secure?: boolean;
  sameSite?: "Strict" | "Lax" | "None";
  path?: string;
  domain?: string;
  maxAge?: number;
  expires?: Date;
}

/** Headers and cookies of the response, before its first byte. */
export interface ResponseWriter {
  readonly headers: Headers;
  readonly cookies: {
    set(name: string, value: string, options?: CookieOptions): void;
    delete(name: string, options?: Pick<CookieOptions, "path" | "domain">): void;
  };
}

export interface LoaderArgs {
  // oxlint-disable-next-line no-explicit-any
  params: any;
  // oxlint-disable-next-line no-explicit-any
  search: any;
  context: AppContext;
  request: Request;
  response: ResponseWriter;
}

export interface MutationArgs {
  context: AppContext;
  request: Request;
  response: ResponseWriter;
}

export interface GuardArgs {
  params: Record<string, string | undefined>;
  url: URL;
  context: AppContext;
  request: Request;
}

/** Coerces string inputs by name: form fields, params, or search params. */
type Fields = (get: (name: string) => (string | File)[], issues: Issue[], lenient: boolean) => Record<string, unknown>;
/** Validates a JSON value and returns it with its typed values restored. */
type Parse = (value: unknown, issues: Issue[]) => unknown;

export interface Loader<T> {
  /** The type of the loader's data, for `RoutePropsFor`. */
  readonly "~data"?: T;
  readonly run?: (args: LoaderArgs) => T | Promise<T>;
  readonly schema?: StandardSchemaV1;
  params?: Fields;
  search?: Fields;
  /** The route file of `loader.external()`, whose data the Rust server loads. */
  external?: string;
}

export interface Mutation<I, R> {
  /** Base-36 hash of the module path and the export name. */
  readonly id: string;
  call(input: I): Promise<R>;
}

interface MutationRecord {
  id: string;
  run: (input: unknown, args: MutationArgs) => unknown;
  schema?: StandardSchemaV1;
  revalidate?: string[];
  form?: Fields;
  json?: Parse;
  call(): Promise<never>;
}

export interface Guard {
  readonly run: (args: GuardArgs) => void | Promise<void>;
}

const isSchema = (value: unknown): value is StandardSchemaV1 => typeof value === "object" && value !== null && "~standard" in value;

/** Declares the loader of a route module, optionally with a schema that validates its result. */
export function loader<T>(run: (args: LoaderArgs) => T | Promise<T>): Loader<Awaited<T>>;
export function loader<T>(schema: StandardSchemaV1<unknown, T>, run: (args: LoaderArgs) => unknown): Loader<T>;
export function loader(first: StandardSchemaV1 | ((args: LoaderArgs) => unknown), second?: (args: LoaderArgs) => unknown): Loader<unknown> {
  return isSchema(first) ? { run: second, schema: first } : { run: first };
}

/** Declares a loader that runs in the Rust server. */
loader.external = <T>(): Loader<T> => ({});

const mutations = new Map<string, MutationRecord>();

/** Declares a server write, a public POST endpoint whose input is validated. */
export function mutation<I, R>(
  run: (input: I, args: MutationArgs) => R | Promise<R>,
  options?: { revalidate?: string[] },
): Mutation<I, Awaited<R>>;
export function mutation<I, R>(
  schema: StandardSchemaV1<unknown, I>,
  run: (input: I, args: MutationArgs) => R | Promise<R>,
  options?: { revalidate?: string[] },
): Mutation<I, Awaited<R>>;
export function mutation(
  first: StandardSchemaV1 | ((input: never, args: MutationArgs) => unknown),
  second?: ((input: never, args: MutationArgs) => unknown) | { revalidate?: string[] },
  third?: { revalidate?: string[] },
): Mutation<never, unknown> {
  const [schema, run, options] = isSchema(first)
    ? [first, second as MutationRecord["run"], third]
    : [undefined, first, second as typeof third];
  const record: MutationRecord = {
    id: "",
    run: run as MutationRecord["run"],
    schema,
    revalidate: options?.revalidate,
    call: () => Promise.reject(new Error("`.call` runs in the browser")),
  };
  return record as unknown as Mutation<never, unknown>;
}

/** Declares the guard of a layout, which runs before every loader and mutation below it. */
export function guardRoute(run: (args: GuardArgs) => void | Promise<void>): Guard {
  return { run };
}

/** A route error: `notFound`, `fail`, or a `500` for anything else thrown. */
export class RouteError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

/** Thrown by `.call` in the browser when the server rejects the input. */
export class MutationInputError extends Error {
  constructor(readonly issues: Issue[]) {
    super("Invalid mutation input");
  }
}

class Redirect {
  constructor(
    readonly location: string,
    readonly status?: number,
  ) {}
}

/** Stops the request with a redirect: `307` from a loader, `303` from a mutation. */
export function redirect(location: string, status?: 301 | 302 | 303 | 307 | 308): never {
  throw new Redirect(location, status);
}

/** Stops the request with `404`. */
export function notFound(): never {
  throw new RouteError(404, "Not Found");
}

/** Stops the request with a `4xx` status. */
export function fail(status: number, message = "Request failed"): never {
  throw new RouteError(status, message);
}

class Deferred {
  settled = false;
  failed = false;
  value: unknown;
  readonly promise: Promise<unknown>;
  constructor(value: unknown) {
    this.promise = Promise.resolve(value).then(
      (resolved) => {
        this.value = resolved;
        this.settled = true;
      },
      (error: unknown) => {
        this.value = error;
        this.settled = this.failed = true;
      },
    );
  }
}

/** Thrown by a read of a deferred field before its value arrives. */
class Pending {
  constructor(readonly promise: Promise<unknown>) {}
}

/** Marks a loader field that streams into its `<Loading>` boundary when it resolves. */
export function defer<T>(value: Promise<T> | T): T {
  return new Deferred(value) as T;
}

/** Loader data whose deferred fields throw `Pending` until they settle, then read as their value. */
function deferrable(value: unknown, pending: Promise<unknown>[]): unknown {
  if (typeof value !== "object" || value === null || !Object.values(value).some((field) => field instanceof Deferred)) return value;
  const out: Record<string, unknown> = {};
  for (const [name, field] of Object.entries(value)) {
    if (!(field instanceof Deferred)) {
      out[name] = field;
      continue;
    }
    pending.push(field.promise);
    Object.defineProperty(out, name, {
      enumerable: true,
      get() {
        if (!field.settled) throw new Pending(field.promise);
        if (field.failed) throw field.value;
        return field.value;
      },
    });
  }
  return out;
}

/** Attaches the compiler's coercion of params and search params. */
export function $load<T>(target: Loader<T>, meta: Pick<Loader<T>, "params" | "search" | "external">): Loader<T> {
  return Object.assign(target, meta);
}

/** Attaches the id and the generated input validators of a mutation, and registers it. */
export function $mut<I, R>(target: Mutation<I, R>, id: string, meta: { form?: Fields; json?: Parse }): Mutation<I, R> {
  const record = Object.assign(target as unknown as MutationRecord, meta, { id });
  mutations.set(id, record);
  return target;
}

const text = /[&<]/g;
const attribute = /[&"]/g;
const entities: Record<string, string> = { "&": "&amp;", "<": "&lt;", '"': "&quot;" };
const replace = (c: string) => entities[c]!;

export const $h = (html: string) => new Markup(html);
export const $l = (render: Thunk) => new Markup(render);

export function $t(value: unknown): string {
  if (value == null || typeof value === "boolean") return "";
  if (value instanceof Markup) {
    return typeof value.content === "string" ? value.content : value.content();
  }
  return String(value as string).replace(text, replace);
}

export function $a(name: string, value: unknown): string {
  if (typeof value === "boolean" && name.startsWith("aria-")) return ` ${name}="${value}"`;
  if (value == null || value === false) return "";
  if (value === true) return ` ${name}`;
  return ` ${name}="${String(value as string).replace(attribute, replace)}"`;
}

/** The `action` of a `<form>`: a mutation posts to the page URL with its id in `_m`. */
export function $fa($r: RenderContext, value: unknown): string {
  const id = (value as Partial<MutationRecord> | null)?.id;
  if (!id || mutations.get(id) !== value) return $a("action", value);
  const search = new URLSearchParams($r.url.search);
  search.delete("_m");
  search.append("_m", id);
  return ` method="post"${$a("action", `${$r.url.pathname}?${search}`)}`;
}

/** `aria-current` and `data-active` of an anchor whose `href` is the current path. */
export function $ac($r: RenderContext, href: unknown): string {
  return typeof href === "string" && href[0] === "/" && href[1] !== "/" && href.split(/[?#]/)[0] === $r.url.pathname
    ? ' aria-current="page" data-active'
    : "";
}

export function $s(props: Record<string, unknown> | null | undefined): string {
  let out = "";
  for (const name in props) {
    if (name === "children" || name === "ref" || /^on[A-Z]/.test(name)) continue;
    out += $a(name, props[name]);
  }
  return out;
}

export function $show<T>(value: T, then: (value: NonNullable<T>) => string, otherwise?: Thunk): string {
  if (value) return then(value as NonNullable<T>);
  return otherwise ? otherwise() : "";
}

export function $for<T>(each: readonly T[] | null | undefined, row: (item: T, index: number) => string, fallback?: Thunk): string {
  if (!each?.length) return fallback ? fallback() : "";
  let out = "";
  for (let index = 0; index < each.length; index++) out += row(each[index]!, index);
  return out;
}

export function $repeat(count: number, row: (index: number) => string, fallback?: Thunk): string {
  if (!(count > 0)) return fallback ? fallback() : "";
  let out = "";
  for (let index = 0; index < count; index++) out += row(index);
  return out;
}

class Writer implements ResponseWriter {
  /** Set once the status is decided: later writes, from deferred fields, throw. */
  sealed = false;
  readonly headers: Headers;
  readonly setCookies: string[] = [];
  readonly cookies = {
    set: (name: string, value: string, options: CookieOptions = {}) => {
      this.check();
      let cookie = `${name}=${encodeURIComponent(value)}; Path=${options.path ?? "/"}`;
      if (options.domain) cookie += `; Domain=${options.domain}`;
      if (options.maxAge !== undefined) cookie += `; Max-Age=${options.maxAge}`;
      if (options.expires) cookie += `; Expires=${options.expires.toUTCString()}`;
      if (options.httpOnly ?? true) cookie += "; HttpOnly";
      if (options.secure ?? true) cookie += "; Secure";
      cookie += `; SameSite=${options.sameSite ?? "Lax"}`;
      this.setCookies.push(cookie);
    },
    delete: (name: string, options: Pick<CookieOptions, "path" | "domain"> = {}) => {
      this.cookies.set(name, "", { ...options, maxAge: 0 });
    },
  };

  constructor() {
    const check = () => this.check();
    this.headers = new (class extends Headers {
      override set(name: string, value: string) {
        check();
        super.set(name, value);
      }
      override append(name: string, value: string) {
        check();
        super.append(name, value);
      }
      override delete(name: string) {
        check();
        super.delete(name);
      }
    })();
  }

  check(): void {
    if (this.sealed) throw new Error("The response has started: a deferred field cannot write headers or cookies");
  }
}

/** The strictest of the segments' `Cache-Control` values. */
function cacheControl(values: string[]): string | null {
  if (values.length === 0) return null;
  const directives = values.map(
    (value) =>
      new Map(
        value.split(",").map((part) => {
          const [name, argument] = part.trim().toLowerCase().split("=");
          return [name!, argument];
        }),
      ),
  );
  if (directives.some((d) => d.has("no-store"))) return "no-store";
  const out: string[] = [];
  if (directives.some((d) => d.has("private"))) out.push("private");
  else if (directives.some((d) => d.has("public"))) out.push("public");
  if (directives.some((d) => d.has("no-cache"))) out.push("no-cache");
  for (const name of ["max-age", "s-maxage", "stale-while-revalidate"]) {
    const ages = directives.flatMap((d) => (d.get(name) === undefined ? [] : [Number(d.get(name))]));
    if (ages.length > 0) out.push(`${name}=${Math.min(...ages)}`);
    else if (directives.some((d) => d.has(name))) out.push(name);
  }
  return out.join(", ");
}

function headersOf(writers: Writer[], init: Record<string, string> = {}): Headers {
  const headers = new Headers(init);
  const caches: string[] = [];
  for (const writer of writers) {
    for (const [name, value] of writer.headers) {
      if (name === "cache-control") caches.push(value);
      else headers.set(name, value);
    }
    for (const cookie of writer.setCookies) headers.append("set-cookie", cookie);
  }
  const cache = cacheControl(caches);
  if (cache) headers.set("cache-control", cache);
  return headers;
}

const builtIn = (status: number, message: string) =>
  `<!doctype html><html><head><meta charset="utf-8"><title>${status}</title></head><body><h1>${status}</h1><p>${$t(message)}</p></body></html>`;

/** What one render needs beyond the route: loader data, failed segments, and the submission. */
interface Page {
  route: { chain: RouteModule[]; chunk: number | null };
  params: Record<string, unknown>;
  url: URL;
  data: unknown[];
  /** The error of the first segment that failed, and its depth. */
  error?: [number, unknown];
  /** The depth of a `render: "client"` page, which renders empty. */
  client?: number;
  submission?: [MutationRecord, Submission];
}

/**
 * Renders the page from segment `from` down: the whole document from `0`, or for a client
 * navigation the document whose body is that segment alone.
 */
function render(
  page: Page,
  assets: Assets | undefined,
  caught: (error: unknown) => RouteError,
  from = 0,
): [string, number, AsyncGenerator<string> | undefined] {
  const { route, params, url } = page;
  const keyed = new Map<string, HeadEntry>([
    ["meta:charset", { depth: -1, html: '<meta charset="utf-8">' }],
    ["meta:name=viewport", { depth: -1, html: '<meta name="viewport" content="width=device-width, initial-scale=1">' }],
  ]);
  const unkeyed: string[] = [];
  let ids = 0;
  let status = 200;
  const entries: unknown[][] = [];
  const events = new Set<string>();
  let eager = false;
  const parts: { children: Thunk; depth: number; promise: Promise<unknown> }[] = [];
  const $r: RenderContext = {
    depth: 0,
    params,
    url,
    head(key, html) {
      if (key === null) {
        if (!unkeyed.includes(html)) unkeyed.push(html);
      } else if ((keyed.get(key)?.depth ?? -2) <= this.depth) {
        keyed.set(key, { depth: this.depth, html });
      }
      return "";
    },
    id: () => `rz-${ids++}`,
    scope(component, names, captures, isEager) {
      for (const name of names.split(" ")) if (name) events.add(name);
      eager ||= isEager;
      return (entries.push([component, ...captures]) - 1).toString(36);
    },
    row(parent, kind, captures) {
      return (entries.push([~parseInt(parent, 36), kind, ...captures]) - 1).toString(36);
    },
    submission(mutation) {
      const submission = page.submission;
      return submission && submission[0] === (mutation as unknown) ? submission[1] : {};
    },
    boundary(children, fallback) {
      const mark = [entries.length, unkeyed.length, new Map(keyed), ids, this.depth] as const;
      try {
        return children();
      } catch (thrown) {
        entries.length = mark[0];
        unkeyed.length = mark[1];
        keyed.clear();
        for (const [key, entry] of mark[2]) keyed.set(key, entry);
        ids = mark[3];
        this.depth = mark[4];
        if (thrown instanceof Pending) throw thrown;
        const error = caught(thrown);
        status = error.status;
        return fallback(error);
      }
    },
    loading(children, fallback) {
      const mark = [entries.length, ids] as const;
      try {
        return children();
      } catch (thrown) {
        if (!(thrown instanceof Pending)) throw thrown;
        entries.length = mark[0];
        ids = mark[1];
        return `<!--l${parts.push({ children, depth: this.depth, promise: thrown.promise }) - 1}-->${fallback()}<!--/l-->`;
      }
    },
  };
  const block = () => {
    if (entries.length === 0 || !assets || route.chunk === null) return "";
    const block = [[...events].join(" "), assets.chunk(route.chunk), eager ? 1 : 0, ...entries];
    entries.length = 0;
    events.clear();
    eager = false;
    return `<script type="rz">${JSON.stringify(block).replaceAll("<", "\\u003c")}</script>`;
  };
  async function* stream(): AsyncGenerator<string> {
    const waiting = new Map(parts.map((part, id) => [id, part.promise.then(() => id)]));
    while (waiting.size > 0) {
      const id = await Promise.race(waiting.values());
      const part = parts[id]!;
      const before = parts.length;
      $r.depth = part.depth;
      let html: string | undefined;
      try {
        html = part.children();
      } catch (thrown) {
        if (thrown instanceof Pending) {
          part.promise = thrown.promise;
          waiting.set(
            id,
            thrown.promise.then(() => id),
          );
          continue;
        }
        caught(thrown);
      }
      waiting.delete(id);
      for (let next = before; next < parts.length; next++)
        waiting.set(
          next,
          parts[next]!.promise.then(() => next),
        );
      if (html !== undefined) yield `<template data-rz-s="${id}">${html}${block()}</template>\n`;
    }
    yield "</body></html>";
  }
  const level = (depth: number): string => {
    if (page.error?.[0] === depth) throw page.error[1];
    const children =
      depth + 1 < route.chain.length || page.error?.[0] === depth + 1
        ? $l(() => {
            const previous = $r.depth;
            $r.depth = depth + 1;
            try {
              return `<!--s${depth + 1}-->${level(depth + 1)}<!--/-->`;
            } finally {
              $r.depth = previous;
            }
          })
        : undefined;
    if (page.client === depth) return "<template data-rz-client></template>";
    return route.chain[depth]!.default({ params, data: page.data[depth], children }, $r);
  };
  $r.depth = from;
  const root = level(from);
  let head = "";
  for (const entry of keyed.values()) head += entry.html;
  head += unkeyed.join("");
  if (assets) head += `<script type="module" async src="${assets.dispatcher.replace(attribute, replace)}"></script>`;
  if (assets && parts.length > 0) head += `<script type="module" async src="${assets.stream.replace(attribute, replace)}"></script>`;
  const document = `<!doctype html><html><head>${head}</head><body>${root}${block()}`;
  return parts.length > 0 ? [document, status, stream()] : [`${document}</body></html>`, status, undefined];
}

export interface HandlerOptions {
  /** Builds the `context` of loaders, guards, and mutations, once per request. */
  context?(request: Request): unknown;
  onRequest?(request: Request): void;
  /** Receives every error that becomes a `500`, with its real message. */
  onError?(error: unknown, request: Request): void;
  /** Origins other than the page's own that may post to mutations. */
  trustedOrigins?: string[];
  /** Byte limits of a mutation body: JSON and form fields, and files. */
  limits?: { body?: number; files?: number };
  /** The origin of the Rust server whose `dev` feature runs the external loaders. */
  loaders?: string;
  /** Writes loader and render durations into `Server-Timing`. On in development, opt-in in production, because it reveals timing to every client. */
  serverTiming?: boolean;
}

export interface AppOptions extends HandlerOptions {
  assets?: Assets;
  dev?: boolean;
  /** The id of the build, which client requests carry in `_b`. */
  build?: string;
}

export type Handler = (request: Request) => Promise<Response>;

const html = { "content-type": "text/html; charset=utf-8" };

/** Whether a mutation post comes from another site, by `Sec-Fetch-Site`, then `Origin`. */
function crossSite(request: Request, trusted: string[]): boolean {
  const origin = request.headers.get("origin");
  if (origin && trusted.includes(origin)) return false;
  const site = request.headers.get("sec-fetch-site");
  if (site) return site === "cross-site";
  return origin !== null && new URL(origin).host !== request.headers.get("host");
}

const tooLarge = () => new RouteError(413, "Content Too Large");

/** Reads the mutation body: JSON for `.call`, form fields otherwise. */
async function body(request: Request, limits: { body: number; files: number }): Promise<{ json: unknown } | { form: FormData }> {
  const length = Number(request.headers.get("content-length") ?? 0);
  if (length > limits.body + limits.files) throw tooLarge();
  if (request.headers.get("content-type")?.includes("application/json")) {
    const source = await request.text();
    if (source.length > limits.body) throw tooLarge();
    try {
      return { json: JSON.parse(source) };
    } catch {
      throw new RouteError(400, "Invalid JSON");
    }
  }
  const form = await request.formData();
  let fields = 0;
  let files = 0;
  for (const [name, value] of form) {
    if (typeof value === "string") fields += name.length + value.length;
    else files += value.size;
  }
  if (fields > limits.body || files > limits.files) throw tooLarge();
  return { form };
}

/** The raw form values: one string per name, or every value of a repeated name. */
function rawInput(form: FormData): Record<string, string | string[]> {
  const out: Record<string, string | string[]> = {};
  for (const name of new Set(form.keys())) {
    const values = form.getAll(name).filter((value): value is string => typeof value === "string");
    out[name] = values.length === 1 ? values[0]! : values;
  }
  return out;
}

async function validate(record: MutationRecord, input: { json: unknown } | { form: FormData }): Promise<[unknown, Issue[]]> {
  const issues: Issue[] = [];
  if (record.schema) {
    const value = "json" in input ? input.json : rawInput(input.form);
    const result = await record.schema["~standard"].validate(value);
    if (!result.issues) return [result.value, issues];
    for (const issue of result.issues) {
      const path = (issue.path ?? []).map((part) => (typeof part === "object" ? part.key : part));
      issues.push({ message: issue.message, path: path.map((part) => (typeof part === "number" ? part : String(part))) });
    }
    return [undefined, issues];
  }
  if ("json" in input) return [record.json!(input.json, issues), issues];
  const form = input.form;
  if (!record.form) throw new RouteError(415, "This mutation takes JSON input");
  return [record.form((name) => form.getAll(name), issues, false), issues];
}

function keyed(issues: Issue[]): Record<string, Issue> {
  const out: Record<string, Issue> = {};
  for (const issue of issues) out[issue.path.join(".")] ??= issue;
  return out;
}

export function createApp(routes: Route[], root: RouteModule[], options: AppOptions = {}): Handler {
  /** The data of an external loader, from the Rust server. */
  const external = async (file: string, params: Record<string, unknown>) => {
    if (!options.loaders) throw new Error("`loader.external()` needs the `loaders` option: the origin of the Rust server");
    const query = new URLSearchParams({ m: file });
    for (const [name, value] of Object.entries(params)) if (value !== undefined) query.append(name, String(value as string));
    const response = await fetch(`${options.loaders}/__reze/load?${query}`);
    if (response.status >= 400 && response.status < 500) throw new RouteError(response.status, await response.text());
    if (!response.ok) throw new Error(`The Rust loader of ${file} failed with ${response.status}`);
    return response.json() as Promise<unknown>;
  };
  const ordered = [...routes].sort(compare);
  const limits = { body: options.limits?.body ?? 1 << 20, files: options.limits?.files ?? 10 << 20 };
  const caught = (request: Request) => (error: unknown) => {
    if (error instanceof RouteError) return error;
    options.onError?.(error, request);
    return new RouteError(500, options.dev && error instanceof Error ? error.message : "Internal Server Error");
  };

  const timing = options.serverTiming ?? options.dev ?? false;

  return async (request) => {
    options.onRequest?.(request);
    const timings: string[] = [];
    const time = (name: string, start: number) => {
      if (timing) timings.push(`${name};dur=${(performance.now() - start).toFixed(1)}`);
    };
    const timed = <T>(name: string, run: () => T): T => {
      const start = performance.now();
      try {
        return run();
      } finally {
        time(name, start);
      }
    };
    const url = new URL(request.url);
    const build = url.searchParams.get("_b");
    const segment = Number(url.searchParams.get("_s"));
    url.searchParams.delete("_b");
    url.searchParams.delete("_s");
    if (build !== null && build !== options.build) return new Response("Conflict", { status: 409 });
    const parts = url.pathname.split("/").filter(Boolean);
    let route: Route | undefined;
    let params: Record<string, string | undefined> = {};
    for (const candidate of ordered) {
      const matched = match(candidate, parts);
      if (!matched) continue;
      route = candidate;
      params = matched[0];
      if (matched[1] !== url.pathname && request.method === "GET") {
        return new Response(null, { status: 308, headers: { location: matched[1] + url.search } });
      }
      break;
    }
    const writers: Writer[] = [];
    const finish = async ([document, status, rest]: [string, number, AsyncGenerator<string> | undefined], headers: Headers) => {
      const head = request.method === "HEAD";
      if (timings.length > 0) headers.set("server-timing", timings.join(", "));
      if (rest) {
        if (head) void rest.return(undefined);
        return new Response(head ? null : streamed(document, rest), { status, headers });
      }
      if (status === 200 && request.method !== "POST") {
        const tag = `"${await digest(document)}"`;
        headers.set("etag", tag);
        if (
          request.headers
            .get("if-none-match")
            ?.split(/\s*,\s*/)
            .includes(tag)
        )
          return new Response(null, { status: 304, headers });
      }
      return new Response(head ? null : document, { status, headers });
    };
    const respond = (page: Page, status = 200) => {
      try {
        const rendered = timed("render", () => render(page, options.assets, caught(request)));
        if (rendered[1] === 200) rendered[1] = status;
        return finish(rendered, headersOf(writers, html));
      } catch (thrown) {
        const error = caught(request)(thrown);
        return new Response(builtIn(error.status, error.message), { status: error.status, headers: headersOf(writers, html) });
      }
    };
    const redirectTo = (target: Redirect, status: number) =>
      new Response(null, { status: target.status ?? status, headers: headersOf(writers, { location: target.location }) });

    if (!route) {
      const page = {
        route: { chain: root, chunk: null },
        params,
        url,
        data: [],
        error: [root.length, new RouteError(404, "Not Found")] as [number, unknown],
      };
      return respond(page);
    }
    const chain = route.chain;
    const from = request.method === "GET" && Number.isInteger(segment) && segment < chain.length ? Math.max(segment, 0) : 0;
    if (request.method !== "GET" && request.method !== "HEAD" && request.method !== "POST") {
      return new Response("Method Not Allowed", { status: 405, headers: { allow: "GET, HEAD, POST" } });
    }
    if (request.method === "POST" && crossSite(request, options.trustedOrigins ?? [])) return new Response("Forbidden", { status: 403 });

    let context: unknown;
    try {
      context = await options.context?.(request);
    } catch (thrown) {
      const error = caught(request)(thrown);
      return new Response(builtIn(error.status, error.message), { status: error.status, headers: html });
    }

    for (const [depth, module] of chain.entries()) {
      if (!module.guard) continue;
      try {
        await module.guard.run({ params, url, context: context as AppContext, request });
      } catch (thrown) {
        if (thrown instanceof Redirect) return redirectTo(thrown, request.method === "POST" ? 303 : 307);
        return respond({ route, params, url, data: [], error: [depth, thrown] });
      }
    }

    let submission: Page["submission"];
    let status = 200;
    if (request.method === "POST") {
      const record = mutations.get(url.searchParams.get("_m") ?? "");
      if (!record) return new Response("Not Found", { status: 404 });
      const writer = new Writer();
      writers.push(writer);
      let input: { json: unknown } | { form: FormData };
      try {
        input = await body(request, limits);
      } catch (thrown) {
        const error = caught(request)(thrown);
        return new Response(error.message, { status: error.status });
      }
      let value: unknown;
      let issues: Issue[];
      try {
        [value, issues] = await validate(record, input);
      } catch (thrown) {
        const error = caught(request)(thrown);
        return new Response(error.message, { status: error.status });
      }
      if (issues.length > 0) {
        if ("json" in input) return Response.json({ issues }, { status: 422 });
        submission = [record, { input: rawInput(input.form), issues: keyed(issues) }];
        status = 422;
      } else {
        let result: unknown;
        try {
          result = await record.run(value, { context: context as AppContext, request, response: writer });
        } catch (thrown) {
          if (thrown instanceof Redirect) return redirectTo(thrown, 303);
          const error = caught(request)(thrown);
          if ("json" in input) return Response.json({ message: error.message }, { status: error.status, headers: headersOf(writers) });
          return respond({ route, params, url, data: [], error: [chain.length - 1, error] });
        }
        if ("json" in input) return Response.json({ result: result ?? null }, { headers: headersOf(writers) });
        const search = new URLSearchParams(url.search);
        search.delete("_m");
        const location = url.pathname + (search.size > 0 ? `?${search}` : "");
        return new Response(null, { status: 303, headers: headersOf(writers, { location }) });
      }
    }

    const data: unknown[] = [];
    const typed: Record<string, unknown> = { ...params };
    const deferred: Promise<unknown>[] = [];
    let error: Page["error"];
    const runLoader = async (load: NonNullable<RouteModule["load"]>) => {
      if (load.external !== undefined) return external(load.external, params);
      if (!load.run) return;
      const writer = new Writer();
      writers.push(writer);
      let own: Record<string, unknown> = params;
      if (load.params) {
        const issues: Issue[] = [];
        own = load.params((name) => (params[name] === undefined ? [] : [params[name]!]), issues, false);
        if (issues.length > 0) throw new RouteError(404, "Not Found");
        Object.assign(typed, own);
      }
      let search: Record<string, unknown> = Object.fromEntries(url.searchParams);
      if (load.search) {
        const issues: Issue[] = [];
        search = load.search((name) => url.searchParams.getAll(name), issues, true);
        if (issues.length > 0) throw new RouteError(400, "Bad Request");
      }
      const value = await load.run({ params: own, search, context: context as AppContext, request, response: writer });
      if (!load.schema) return value;
      const result = await load.schema["~standard"].validate(value);
      if (result.issues) throw new Error(`The loader result is invalid: ${result.issues.map((issue) => issue.message).join(", ")}`);
      return result.value;
    };
    const run = async (depths: number[]): Promise<Response | undefined> => {
      const results = await Promise.allSettled(
        depths.map(async (depth) => {
          const load = chain[depth]!.load;
          if (!load) return;
          const start = performance.now();
          try {
            return await runLoader(load);
          } finally {
            time(`loader-${depth}`, start);
          }
        }),
      );
      for (const [index, result] of results.entries()) {
        const depth = depths[index]!;
        if (result.status === "fulfilled") {
          data[depth] = deferrable(result.value, deferred);
          continue;
        }
        if (result.reason instanceof Redirect) return redirectTo(result.reason, 307);
        if (!error || error[0] > depth) error = [depth, result.reason];
      }
    };
    const client = submission || build !== null || chain.at(-1)!.render !== "client" ? undefined : chain.length - 1;
    const depths = chain.map((_, depth) => depth);
    const redirected = await run(depths.slice(from).filter((depth) => depth !== client));
    if (redirected) return redirected;
    if (request.headers.has("rz-prerender")) await Promise.all(deferred);
    for (const writer of writers) writer.sealed = true;
    const page = (): Page => ({ route, params: typed, url, data, error, submission, client });
    const revalidate = Math.min(...chain.map((module) => module.revalidate ?? Infinity));
    if (revalidate !== Infinity && request.method !== "POST") {
      const writer = new Writer();
      writer.headers.set("cache-control", `s-maxage=${revalidate}, stale-while-revalidate`);
      writers.push(writer);
    }
    if (from > 0) {
      let rendered: ReturnType<typeof render> | undefined;
      try {
        rendered = timed("render", () => render(page(), options.assets, caught(request), from));
      } catch {
        const again = await run(depths.slice(0, from));
        if (again) return again;
      }
      if (rendered) {
        const headers = headersOf(writers, html);
        headers.set("rz-segment", String(from));
        return finish(rendered, headers);
      }
    }
    return respond(page(), status);
  };
}

/** The URLs to prerender: static routes and routes whose page exports `prerender = true`, once for each of their `entries`. */
export async function prerendered(routes: Route[]): Promise<string[]> {
  const out: string[] = [];
  for (const route of routes) {
    const page = route.chain.at(-1)!;
    if (page.prerender === false || !(page.prerender || (route.static && !route.chain.some((module) => module.guard)))) continue;
    const dynamic = route.segments.some((segment) => segment.kind !== "static");
    for (const params of dynamic ? ((await page.entries?.()) ?? []) : [{}]) {
      const parts = route.segments.flatMap((segment) => {
        if (segment.kind === "static") return [segment.name];
        const value = params[segment.name];
        if (value === undefined) return [];
        return segment.kind === "splat" ? value.split("/").map(encodeURIComponent) : [encodeURIComponent(value)];
      });
      out.push(`/${parts.join("/")}`);
    }
  }
  return out;
}

/** The response body of a streamed page: the document up to its deferred parts, then each part. */
function streamed(document: string, rest: AsyncGenerator<string>): ReadableStream<Uint8Array> {
  const encoder = new TextEncoder();
  let started = false;
  return new ReadableStream({
    async pull(controller) {
      if (!started) {
        started = true;
        controller.enqueue(encoder.encode(document));
        return;
      }
      const next = await rest.next();
      if (next.done) controller.close();
      else controller.enqueue(encoder.encode(next.value));
    },
    cancel() {
      void rest.return(undefined);
    },
  });
}

async function digest(text: string): Promise<string> {
  const hash = new Uint8Array(await crypto.subtle.digest("SHA-1", new TextEncoder().encode(text)));
  return btoa(String.fromCharCode(...hash.subarray(0, 12)))
    .replaceAll("+", "-")
    .replaceAll("/", "_");
}
