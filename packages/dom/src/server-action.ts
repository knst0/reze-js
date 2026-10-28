export interface InputSchema<T> {
  parse(value: unknown): T;
}

export interface ServerActionContext {
  readonly request?: Request;
}

export interface ServerAction<TInput, TOutput> {
  /** Stable module-local id the compiler assigned to this call. */
  readonly id: string;
  /** Path of the POST endpoint, for a `<form method="post" action={…}>` that works without JavaScript. */
  readonly endpoint: string;
  call(input: TInput): Promise<TOutput>;
}

interface Registered {
  readonly schema: InputSchema<unknown>;
  readonly run: (input: unknown, context: ServerActionContext) => unknown;
}

const actions = new Map<string, Registered>();
const ActionPath = "/_reze/action/";
const MaxBodyBytes = 1 << 20;
const isServer = typeof window === "undefined";

function basePath(): string {
  const url = (import.meta as ImportMeta & { env?: { BASE_URL?: string } }).env?.BASE_URL ?? "/";
  return url.replace(/\/$/, "");
}

function endpointOf(id: string): string {
  return `${basePath()}${ActionPath}${encodeURIComponent(id)}`;
}

/**
 * Declares a server mutation. The compiler inserts the id and rejects the call when the first
 * argument is not a schema, because every action is a public POST endpoint and must validate its
 * input. The handler must authorize the caller itself.
 */
export function serverAction<I, R>(
  id: string,
  schema: InputSchema<I>,
  run: (input: I, context: ServerActionContext) => R | PromiseLike<R>,
): ServerAction<I, Awaited<R>> {
  if (typeof schema?.parse !== "function") {
    throw new Error("[reze] server action " + id + " needs an input schema as its first argument");
  }
  const endpoint = endpointOf(id);
  if (isServer) {
    actions.set(id, { schema, run: run as Registered["run"] });
    return {
      id,
      endpoint,
      call: async (input): Promise<Awaited<R>> => (await run(schema.parse(input), {})) as Awaited<R>,
    };
  }
  return {
    id,
    endpoint,
    call: async (input): Promise<Awaited<R>> => {
      const response = await fetch(endpoint, {
        method: "POST",
        credentials: "same-origin",
        headers: { "content-type": "application/json", accept: "application/json" },
        body: JSON.stringify(schema.parse(input)),
      });
      const payload = (await response.json().catch(() => undefined)) as
        | { ok?: boolean; result?: Awaited<R>; error?: string }
        | undefined;
      if (!response.ok || payload?.ok !== true) {
        throw new Error(payload?.error ?? `[reze] server action ${id} failed with ${response.status}`);
      }
      return payload.result as Awaited<R>;
    },
  };
}

function formObject(form: FormData): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [key, value] of form) {
    if (key.startsWith("_")) continue;
    if (typeof value !== "string") continue;
    const existing = out[key];
    if (existing === undefined) out[key] = value;
    else if (Array.isArray(existing)) existing.push(value);
    else out[key] = [existing, value];
  }
  return out;
}

function redirectTarget(form: FormData): string {
  const value = form.get("_return");
  return typeof value === "string" && value.startsWith("/") && !value.startsWith("//") ? value : "/";
}

function errorRedirect(target: string): string {
  const url = new URL(target, "http://reze.invalid");
  url.searchParams.set("_action_error", "1");
  return url.pathname + url.search + url.hash;
}

/** Answers a `POST /_reze/action/<id>`; `undefined` when the request is not an action. */
export async function handleServerAction(request: Request): Promise<Response | undefined> {
  const pathname = new URL(request.url).pathname;
  const at = pathname.lastIndexOf(ActionPath);
  if (at < 0) return undefined;
  if (request.method !== "POST") return new Response("Method Not Allowed", { status: 405 });
  const length = Number(request.headers.get("content-length") ?? "0");
  if (length > MaxBodyBytes) return new Response("Payload Too Large", { status: 413 });
  let id: string;
  try {
    id = decodeURIComponent(pathname.slice(at + ActionPath.length));
  } catch {
    return new Response("Not Found", { status: 404 });
  }
  const action = actions.get(id);
  if (action === undefined) {
    return new Response("Not Found", { status: 404, headers: { "cache-control": "no-store" } });
  }
  const contentType = request.headers.get("content-type") ?? "";
  const isForm = contentType.includes("application/x-www-form-urlencoded") || contentType.includes("multipart/form-data");
  let input: unknown;
  let form: FormData | undefined;
  try {
    if (isForm) {
      form = await request.formData();
      input = formObject(form);
    } else if (contentType.includes("application/json")) {
      input = await request.json();
    } else {
      return new Response("Unsupported Media Type", { status: 415 });
    }
  } catch {
    return new Response("Bad Request", { status: 400 });
  }
  let parsed: unknown;
  try {
    parsed = action.schema.parse(input);
  } catch (error) {
    if (form !== undefined) {
      return new Response(null, { status: 303, headers: { location: errorRedirect(redirectTarget(form)) } });
    }
    return Response.json(
      { ok: false, error: error instanceof Error ? error.message : "invalid input" },
      { status: 422, headers: { "cache-control": "no-store" } },
    );
  }
  try {
    const result = await action.run(parsed, { request });
    if (form !== undefined) {
      return new Response(null, { status: 303, headers: { location: redirectTarget(form) } });
    }
    return Response.json({ ok: true, result: result ?? null }, { headers: { "cache-control": "no-store" } });
  } catch {
    if (form !== undefined) {
      return new Response(null, { status: 303, headers: { location: errorRedirect(redirectTarget(form)) } });
    }
    return new Response("Internal Server Error", { status: 500, headers: { "cache-control": "no-store" } });
  }
}
