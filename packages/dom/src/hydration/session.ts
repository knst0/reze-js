import { getOwner, onCleanup } from "@rezejs/signals";
import type { ContinuationHandle } from "@rezejs/signals/internal/continuation";
import type { SourceSite } from "@rezejs/signals/internal/scope";

import { ClaimIndex } from "./claim";
import { currentExecution, moduleExecution } from "./execution";
import { DirtyForms } from "./forms";
import { ClaimPlan, type PlanHandle, type RangePlan } from "./plan";
import { HydrationError, parsePayload, type HydrationPayload } from "./protocol";
import { HydrationReplay } from "./replay";
import { CommitStaging } from "./staging";
import type { FrameDecoder } from "./codec";

export interface HydrationOptions {
  readonly bootstrapUrl?: string;
  readonly base?: string;
}

const sessions = new WeakMap<Element, HydrationSession>();
const preparedHandles = new WeakMap<PlanHandle, HydrationSession>();

export class HydrationSession extends HydrationReplay {
  readonly claims: ClaimPlan;
  readonly staging: CommitStaging;
  readonly portals: RangePlan[] = [];
  private readonly bindings: (() => void)[] = [];
  private readonly location: string;
  private mounted: RangePlan | undefined;
  private released = false;

  constructor(readonly element: Element, payload: HydrationPayload, decoder: FrameDecoder, readonly base: string) {
    const index = new ClaimIndex(element, payload.layout);
    super(payload, decoder);
    this.claims = new ClaimPlan(index);
    this.staging = new CommitStaging(this);
    this.location = element.ownerDocument.location.href;
  }

  attach(handle: PlanHandle): void {
    preparedHandles.set(handle, this);
  }

  mount(plan: RangePlan): void {
    if (this.mounted !== undefined) throw new HydrationError("hydration root is already mounted");
    this.beginView();
    this.mounted = plan;
  }

  deferCommit(fn: () => void | (() => void)): void {
    this.staging.defer(fn);
  }

  adoptBinding(fn: () => void): void {
    this.bindings.push(fn);
  }

  commit(): void {
    this.check();
    if (this.mounted === undefined) throw new HydrationError("hydration has no root view");
    if (this.element.ownerDocument.location.href !== this.location) throw new HydrationError("page location changed during hydration preparation");
    const roots = [this.mounted];
    for (const portal of this.portals) if (!portal.instance.retired) roots.push(portal);
    this.claims.validate(roots);
    const elements = new Set<Element>([this.element]);
    for (const root of this.claims.index.roots.values()) elements.add(root.element);
    const forms = new DirtyForms(elements);
    this.claims.index.unwrapPortals();
    this.claims.commitText(forms);
    this.staging.commit(forms);
    for (const adopt of this.bindings) adopt();
    this.beginCommit();
    this.staging.commitEffects();
    this.activate();
    this.releaseClaims();
  }

  override dispose(): void {
    try {
      super.dispose();
    } finally {
      this.releaseClaims();
      if (sessions.get(this.element) === this) sessions.delete(this.element);
    }
  }

  private releaseClaims(): void {
    if (this.released) return;
    this.released = true;
    for (const handle of this.claims.handles.keys()) preparedHandles.delete(handle);
    this.claims.release();
    this.claims.index.release();
    this.staging.release();
    this.bindings.length = 0;
    this.portals.length = 0;
    this.mounted = undefined;
  }
}

export function preparingSession(): HydrationSession | undefined {
  const session = currentExecution();
  return session instanceof HydrationSession && session.preparing ? session : undefined;
}

export function sessionFor(handle: PlanHandle): HydrationSession | undefined {
  const session = preparedHandles.get(handle);
  return session?.preparing ? session : undefined;
}

export function stagedSession(element: Element): HydrationSession | undefined {
  return sessions.get(element);
}

export function prepareHydration(element: Element, options: HydrationOptions = {}): HydrationSession {
  if (sessions.has(element)) throw new HydrationError("mount root already has a hydration session");
  const document = element.ownerDocument;
  let script: HTMLScriptElement | undefined;
  for (const candidate of document.querySelectorAll<HTMLScriptElement>('script[type="application/json"][data-reze-state]')) {
    if (candidate.getAttribute("data-reze-state") !== element.id) continue;
    if (script !== undefined) throw new HydrationError("duplicate hydration state script");
    script = candidate;
  }
  if (script === undefined) throw new HydrationError("missing hydration state script");
  const { payload, decoder } = parsePayload(script.textContent ?? "");
  if (payload.rootId !== element.id || document.getElementById(payload.rootId) !== element) throw new HydrationError("hydration mount root does not match payload");
  let base = options.base ?? "/";
  const relativeBase = base === "" || base === "./";
  let pathname = payload.pathname;
  if (relativeBase && options.bootstrapUrl === undefined) throw new HydrationError("relative hydration base requires a bootstrap URL");
  if (options.bootstrapUrl !== undefined) {
    const bootstrap = new URL(options.bootstrapUrl, document.baseURI);
    let found = false;
    for (const candidate of document.querySelectorAll<HTMLScriptElement>('script[type="module"][src]')) {
      if (candidate.src === bootstrap.href) found = true;
    }
    const buildFile = payload.buildId.split("/").map(encodeURIComponent).join("/");
    if (!found || !bootstrap.pathname.endsWith(`/${buildFile}`)) throw new HydrationError("hydration bootstrap buildId does not match the running module");
    if (relativeBase) {
      if (bootstrap.origin !== document.location.origin) throw new HydrationError("relative hydration bootstrap must share the page origin");
      base = bootstrap.pathname.slice(0, -buildFile.length - 1);
      pathname = base + pathname;
    }
  }
  if (document.location.pathname !== new URL(pathname, document.location.href).pathname) throw new HydrationError("hydration page pathname does not match payload");
  const session = new HydrationSession(element, payload, decoder, base);
  sessions.set(element, session);
  if (getOwner() !== undefined) onCleanup(() => session.dispose());
  return session;
}

export function markModule(moduleId: string): void {
  const session = moduleExecution(moduleId);
  if (session instanceof HydrationSession && session.preparing) session.instances.modules.add(moduleId);
}

export function willReplayAwait(handle: ContinuationHandle, site: SourceSite): boolean {
  return preparingSession()?.expectsAwait(handle, site) ?? false;
}

export function replayAwaitOperand(handle: ContinuationHandle, site: SourceSite): undefined {
  return preparingSession()?.replayAwaitOperand(handle, site);
}
