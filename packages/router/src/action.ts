// Ported from @solidjs/router (MIT, Copyright (c) 2020-2022 Ryan Carniato).
import { getOwner, onCleanup } from "@rezejs/signals";

import { setRouterFormHandler } from "./events";
import {
  cacheKeyOp,
  hashKey,
  query,
  readRevalidateKeys,
  revalidate,
  SingleFlightHeader,
  RevalidateHeader,
} from "./query";
import { useRouter, type RouterState } from "./routing";
import type { NarrowResponse, Submission } from "./types";
import { mockBase } from "./utils";

const ActionUrlPrefix = "https://action/";
const LocationHeader = "Location";
const RedirectStatuses = new Set([301, 302, 303, 307, 308]);

const submitHooksKey = Symbol("routerActionSubmitHooks");
const settledHooksKey = Symbol("routerActionSettledHooks");
const invokeKey = Symbol("routerActionInvoke");

interface ActionThis {
  r?: RouterState;
  f?: HTMLFormElement;
}

type SubmitHook = (...args: unknown[]) => void;
type SettledHook = (submission: Submission<unknown, unknown>) => void;
type InvokeFn = (this: ActionThis, variables: unknown[], current: ActionRecord) => Promise<unknown>;

interface ActionRecord {
  (...args: never[]): Promise<unknown>;
  url: string;
  base: string;
  run: (...args: unknown[]) => Promise<unknown>;
  [submitHooksKey]: Map<symbol, SubmitHook>;
  [settledHooksKey]: Map<symbol, SettledHook>;
  [invokeKey]: InvokeFn;
}

export type Action<T extends unknown[], U, V = T> = ((
  ...variables: T
) => Promise<NarrowResponse<U>>) & {
  url: string;
  with<A extends unknown[], B extends unknown[]>(...args: A): Action<B, U, V>;
  onSubmit(hook: (...args: V extends unknown[] ? V : T) => void): Action<T, U, V>;
  onSettled(
    hook: (submission: Submission<V extends unknown[] ? V : T, NarrowResponse<U>>) => void,
  ): Action<T, U, V>;
};

const actions = new Map<string, ActionRecord>();

const busyForms = new WeakMap<HTMLFormElement, number>();

function setFormBusy(form: HTMLFormElement, delta: number): void {
  const count = (busyForms.get(form) ?? 0) + delta;
  busyForms.set(form, count);
  if (count > 0) form.setAttribute("aria-busy", "true");
  else form.removeAttribute("aria-busy");
}

function setFunctionName(fn: object, name: string): void {
  Object.defineProperty(fn, "name", { value: name, writable: false, configurable: false });
}

let integrationsInstalled = false;

function installRouterIntegrations(): void {
  if (integrationsInstalled) return;
  integrationsInstalled = true;
  if (typeof window !== "undefined") setRouterFormHandler(handleFormAction);
}

function toSearchParams(data: FormData): URLSearchParams {
  const params = new URLSearchParams();
  for (const [key, value] of data)
    params.append(key, typeof value === "string" ? value : value.name);
  return params;
}

/** Applies a mutation response's metadata: revalidation keys, redirects, single-flight payload. */
function applyResponseMetadata(
  headers: Headers,
  status: number,
  flight: Record<string, unknown> | undefined,
  navigate: ((to: string, options?: { replace?: boolean }) => void) | undefined,
): void {
  const keys = headers.has(RevalidateHeader)
    ? readRevalidateKeys(headers.get(RevalidateHeader) as string)
    : undefined;
  const redirectTo = RedirectStatuses.has(status) ? headers.get(LocationHeader) : null;
  if (redirectTo !== null && typeof window !== "undefined") {
    if (navigate && redirectTo.startsWith("/")) navigate(redirectTo, { replace: true });
    else window.location.href = redirectTo;
  }
  cacheKeyOp(keys, (entry) => void (entry.stamp = 0));
  if (flight) {
    for (const key of Object.keys(flight)) query.set(key, flight[key]);
  }
  revalidate(keys, false);
}

interface ActionOutcome {
  data?: unknown;
  error?: unknown;
}

async function handleActionResponse(
  response: unknown,
  isError: boolean,
  router: RouterState | undefined,
): Promise<ActionOutcome | undefined> {
  const navigate = router?.navigatorFactory();
  if (response instanceof Response) {
    let data: unknown = response;
    let flight: Record<string, unknown> | undefined;
    const contentType = response.headers.get("content-type") ?? "";
    if (response.headers.has(SingleFlightHeader) || contentType.includes("json")) {
      try {
        const body = (await response.json()) as unknown;
        const envelope = body as { flight?: unknown; data?: unknown };
        if (typeof envelope.flight === "object" && envelope.flight !== null) {
          flight = envelope.flight as Record<string, unknown>;
          data = envelope.data;
        } else {
          data = body;
        }
      } catch {
        data = response.status === 204 ? undefined : response;
      }
    } else if (response.status === 204 || response.body === null) {
      data = undefined;
    }
    applyResponseMetadata(response.headers, response.status, flight, navigate);
    return data != null ? { data } : undefined;
  }
  if (isError) return { error: response };
  return response != null ? { data: response } : undefined;
}

async function invoke(
  this: ActionThis,
  variables: unknown[],
  current: ActionRecord,
): Promise<unknown> {
  const router = this?.r;
  const form = this?.f;
  const submitHooks = current[submitHooksKey];
  const settledHooks = current[settledHooksKey];
  if (submitHooks.size) {
    for (const hook of submitHooks.values()) hook(...variables);
  }
  if (form) setFormBusy(form, 1);
  let outcome: ActionOutcome | undefined;
  try {
    let raw: unknown;
    let failed = false;
    try {
      raw = await current.run(...variables);
    } catch (error: unknown) {
      failed = true;
      raw = error;
    }
    outcome = await handleActionResponse(raw, failed, router);
  } finally {
    if (form) setFormBusy(form, -1);
  }
  const submission: Submission<unknown, unknown> = {
    input: variables,
    url: current.url,
    result: outcome?.data as never,
    error: outcome?.error,
    clear: () => {
      router?.submissions[1]((entries) => entries.filter((candidate) => candidate !== submission));
    },
    retry: () => {
      submission.clear();
      return current[invokeKey].call({ r: router, f: form }, variables, current);
    },
  };
  if (outcome && router) router.submissions[1]((entries) => [...entries, submission]);
  for (const hook of settledHooks.values()) hook(submission);
  if (outcome) {
    if (outcome.error && !form) throw outcome.error;
    return outcome.data;
  }
  return undefined;
}

const hashString = (value: string): number =>
  value.split("").reduce((hash, char) => ((hash << 5) - hash + char.charCodeAt(0)) | 0, 0);

function toAction(
  run: (...args: unknown[]) => Promise<unknown>,
  url: string,
  boundArgs: unknown[] = [],
  base: string = url,
  submitHooks: Map<symbol, SubmitHook> = new Map(),
  settledHooks: Map<symbol, SettledHook> = new Map(),
): ActionRecord {
  const fn = function (this: ActionThis, ...args: unknown[]): Promise<unknown> {
    return invoke.call(this, [...boundArgs, ...args], fn as unknown as ActionRecord);
  } as unknown as ActionRecord;
  fn.toString = () => {
    if (!url) throw new Error("Client Actions need explicit names if server rendered");
    return url;
  };
  (fn as unknown as Action<unknown[], unknown>).with = (...args: unknown[]) => {
    const uri = new URL(url, mockBase);
    uri.searchParams.set("args", hashKey(args));
    const boundUrl =
      (uri.origin === "https://action" ? uri.origin : "") + uri.pathname + uri.search;
    return toAction(
      run,
      boundUrl,
      [...boundArgs, ...args],
      base,
      submitHooks,
      settledHooks,
    ) as never;
  };
  (fn as unknown as Action<unknown[], unknown>).onSubmit = (hook) => {
    const id = Symbol("actionOnSubmitHook");
    submitHooks.set(id, hook);
    if (getOwner()) onCleanup(() => void submitHooks.delete(id));
    return fn as never;
  };
  (fn as unknown as Action<unknown[], unknown>).onSettled = (hook) => {
    const id = Symbol("actionOnSettledHook");
    settledHooks.set(id, hook as SettledHook);
    if (getOwner()) onCleanup(() => void settledHooks.delete(id));
    return fn as never;
  };
  fn.url = url;
  fn.base = base;
  fn[submitHooksKey] = submitHooks;
  fn[settledHooksKey] = settledHooks;
  fn[invokeKey] = invoke;
  fn.run = run;
  installRouterIntegrations();
  if (typeof window !== "undefined" && url) {
    actions.set(url, fn);
    if (getOwner()) onCleanup(() => void (actions.get(url) === fn && actions.delete(url)));
  }
  return fn;
}

export function action<T extends unknown[], U = void>(
  fn: (...args: T) => Promise<U>,
  name?: string,
): Action<T, U>;
export function action<T extends unknown[], U = void>(
  fn: (...args: T) => Promise<U>,
  options?: { name?: string },
): Action<T, U>;
export function action<T extends unknown[], U>(
  fn: (...args: T) => Promise<U>,
  nameOrOptions?: string | { name?: string },
): Action<T, U> {
  const options =
    typeof nameOrOptions === "string" ? { name: nameOrOptions } : (nameOrOptions ?? {});
  const name =
    options.name ?? (typeof window !== "undefined" ? String(hashString(fn.toString())) : undefined);
  const url = (fn as { url?: string }).url ?? (name ? `${ActionUrlPrefix}${name}` : "");
  const wrapped = toAction(
    (...args: unknown[]) => (fn as (...args: unknown[]) => Promise<U>)(...args),
    url,
  );
  if (name) setFunctionName(wrapped, name);
  return wrapped as unknown as Action<T, U>;
}

/** Binds direct action invocation to the current router context. */
export function useAction<T extends unknown[], U, V>(
  fn: Action<T, U, V>,
): (...args: T) => Promise<NarrowResponse<U>> {
  const router = useRouter();
  return (...args) =>
    (fn as unknown as ActionRecord)[invokeKey].call(
      { r: router },
      args as unknown[],
      fn as unknown as ActionRecord,
    ) as Promise<NarrowResponse<U>>;
}

/**
 * The action's settled records (results or errors only), filtered by `filter` on the input.
 * Read reactively; each record clears itself or retries with the same input.
 */
export function useSubmissions<T extends unknown[], U, V>(
  fn: Action<T, U, V>,
  filter?: (input: V) => boolean,
): Submission<V, NarrowResponse<U>>[] {
  const router = useRouter();
  const base = (fn as unknown as ActionRecord).base;
  const read = (): Submission<V, NarrowResponse<U>>[] =>
    (router.submissions[0]() as Submission<unknown, unknown>[]).filter(
      (submission): submission is Submission<V, NarrowResponse<U>> =>
        submission.url === base && (!filter || filter(submission.input as V)),
    );
  return new Proxy([] as Submission<V, NarrowResponse<U>>[], {
    get(_, property) {
      const items = read();
      const value = (items as unknown as Record<PropertyKey, unknown>)[property];
      return typeof value === "function" ? (value as () => unknown).bind(items) : value;
    },
    has(_, property) {
      return property in read();
    },
  });
}

/** The document-delegation submit handler for router actions. */
export function handleFormAction(
  event: SubmitEvent,
  router: RouterState,
  actionBase: string,
): void {
  if (event.defaultPrevented) return;
  const form = event.target as HTMLFormElement;
  const submitter = event.submitter as HTMLElement | null;
  const ref =
    (submitter?.hasAttribute("formaction") ? submitter.getAttribute("formaction") : null) ??
    form.getAttribute("action");
  if (!ref) return;
  const isServerAction = !ref.startsWith(ActionUrlPrefix);
  let actionRef = ref;
  if (isServerAction) {
    const url = new URL(ref, mockBase);
    actionRef = router.parsePath(url.pathname + url.search);
    if (!actionRef.startsWith(actionBase)) return;
  }
  if (form.method.toUpperCase() !== "POST")
    throw new Error("Only POST forms are supported for Actions");
  let handler = actions.get(actionRef);
  if (!handler && isServerAction) {
    handler = toAction((body: unknown) => {
      const headers = new Headers();
      if (router.singleFlight) headers.set(SingleFlightHeader, "1");
      return fetch(actionRef, {
        method: "POST",
        headers,
        body: body as FormData | URLSearchParams,
      });
    }, actionRef);
    actions.set(actionRef, handler);
  }
  if (!handler) return;
  event.preventDefault();
  const data = new FormData(form, submitter);
  void invoke.call(
    { r: router, f: form },
    [form.enctype === "multipart/form-data" ? data : toSearchParams(data)],
    handler,
  );
}

/**
 * Entry point for delegation's lazy fallback: when no action module is in the client graph,
 * the router intercepts posts to action urls synchronously and loads this module to run them.
 */
export function submitServerForm(
  router: RouterState,
  url: string,
  form: HTMLFormElement,
  data: FormData | URLSearchParams,
): void {
  const handler = actions.get(url);
  if (!handler) {
    form.submit();
    return;
  }
  void invoke.call({ r: router, f: form }, [data], handler);
}
