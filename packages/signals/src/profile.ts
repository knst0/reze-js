import type { ReactiveNode } from "./graph";

export type ProfileNodeKind = "signal" | "computed" | "effect" | "render" | "root" | "async";
export type ProfileEventType = "created" | "rerun" | "write" | "dispose";

export interface ProfileEvent {
  type: ProfileEventType;
  kind: ProfileNodeKind;
  name: string | undefined;
  id: number;
  component: string | undefined;
  file: string | undefined;
}

export interface ProfileHook {
  event(event: ProfileEvent): void;
}

export let profileHook: ProfileHook | undefined;

/** Installs the attribution sink a dev session or registry reads; `undefined` removes it. */
export function setProfileHook(hook: ProfileHook | undefined): void {
  profileHook = hook;
}

interface Tracked {
  kind: ProfileNodeKind;
  name: string | undefined;
  id: number;
  component: string | undefined;
  file: string | undefined;
}

interface Scope {
  name: string;
  file: string;
}

let nextId = 0;
let tracked: WeakMap<ReactiveNode, Tracked> | undefined;
let scopes: Scope[] | undefined;

function currentScope(): Scope | undefined {
  return scopes !== undefined && scopes.length > 0 ? scopes[scopes.length - 1] : undefined;
}

function emit(node: ReactiveNode, type: ProfileEventType): void {
  const hook = profileHook;
  const info = tracked?.get(node);
  if (hook === undefined || info === undefined) return;
  hook.event({
    type,
    kind: info.kind,
    name: info.name,
    id: info.id,
    component: info.component,
    file: info.file,
  });
}

/** Records `node` for later attribution; dev-only, call inside a compile-time dev check. */
export function profileCreated(node: ReactiveNode, kind: ProfileNodeKind, name: string | undefined): void {
  const scope = currentScope();
  (tracked ??= new WeakMap()).set(node, { kind, name, id: nextId++, component: scope?.name, file: scope?.file });
  emit(node, "created");
}

/** `node` re-evaluated after its first run; call at genuine re-runs only, never the first run. */
export function profileReran(node: ReactiveNode): void {
  emit(node, "rerun");
}

/** A signal was written a value different from its previous one. */
export function profileWrote(node: ReactiveNode): void {
  emit(node, "write");
}

export function profileDisposed(node: ReactiveNode): void {
  emit(node, "dispose");
}

function splitScopeId(id: string): string {
  const at = id.lastIndexOf("#");
  return at < 0 ? "" : id.slice(0, at);
}

/** Runs `run` with nodes it creates attributed to `name` in `id` (`"file#Component"). */
export function profileComponent<T>(name: string, id: string | undefined, props: number, run: () => T): T {
  const file = id === undefined ? "" : splitScopeId(id);
  const stack = (scopes ??= []);
  stack.push({ name, file });
  if (session !== undefined) {
    const row = sessionRow(name, file);
    row.mounts += 1;
    if (props > row.props) row.props = props;
  }
  try {
    return run();
  } finally {
    stack.pop();
  }
}

export interface ProfileComponentFacts {
  component: string;
  file: string;
  mounts: number;
  props: number;
  reruns: number;
  writes: number;
}

export interface ProfileTree {
  v: 1;
  components: ProfileComponentFacts[];
}

let session: Map<string, ProfileComponentFacts> | undefined;
let previousHook: ProfileHook | undefined;

function sessionRow(component: string, file: string): ProfileComponentFacts {
  const rows = session!;
  const key = `${file}#${component}`;
  let row = rows.get(key);
  if (row === undefined) {
    row = { component, file, mounts: 0, props: 0, reruns: 0, writes: 0 };
    rows.set(key, row);
  }
  return row;
}

const sessionHook: ProfileHook = {
  event(event) {
    if (session !== undefined) {
      if (event.type === "rerun") {
        sessionRow(event.component ?? "", event.file ?? "").reruns += 1;
      } else if (event.type === "write") {
        sessionRow(event.component ?? "", event.file ?? "").writes += 1;
      }
    }
    previousHook?.event(event);
  },
};

/** Installs the session collector, forwarding to any hook already installed. */
export function startProfileSession(): void {
  if (session !== undefined) return;
  session = new Map();
  previousHook = profileHook;
  profileHook = sessionHook;
}

/** Removes the session collector and returns its tree, safe to serialize. */
export function stopProfileSession(): ProfileTree {
  const rows = session;
  session = undefined;
  profileHook = previousHook;
  previousHook = undefined;
  const components = rows === undefined ? [] : [...rows.values()];
  components.sort((a, b) => (a.file < b.file ? -1 : a.file > b.file ? 1 : a.component < b.component ? -1 : 1));
  return { v: 1, components };
}

/** Stops the session and best-effort POSTs its tree to the dev server; never throws. */
export async function submitProfileSession(url = "/__reze/profile"): Promise<ProfileTree> {
  const tree = stopProfileSession();
  if (process.env.NODE_ENV !== "production") {
    await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(tree),
    }).catch(() => undefined);
  }
  return tree;
}
