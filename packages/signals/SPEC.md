# Signals scheduling and ownership semantics

This document is normative for the public API exported by
`packages/signals/src/index.ts`. `Guaranteed` rules are part of the public
contract. `Unspecified` rules are deliberately not part of the contract:
callers must not rely on them, and a future release may change them without
treating the change as breaking. Anchors cite the implementation or test that
proves each rule today; the anchor does not widen the guarantee beyond the
sentence it supports.

## Scheduling and flush order

- **Guaranteed — FIFO queue.** Queued watching nodes run in the order they
  were enqueued. A node that is already queued is not enqueued again until it
  runs, so repeated writes before a flush coalesce into one queued run.
  Anchor: `src/scheduler.ts` `enqueueGlobal`/`flushGlobal` (the `queued`
  array, `notifyIndex`/`queuedLength`, and clearing `FlagWatching` on
  enqueue clears duplicate queueing); `tests/scheduler-t16.spec.ts`
  "watching owners execute outermost first" for queue order.
- **Guaranteed — Outermost owner first.** When a watching node and its
  watching owners are queued by one scheduling step, the outermost owner
  runs first, then each inner owner, then the node itself.
  Anchor: `src/scheduler.ts` `scheduleNode`/`enqueueGlobal` (walks
  `subs?.sub` watching owners and reverses the inserted chain);
  `tests/scheduler-t16.spec.ts` "watching owners execute outermost first"
  expects `["outer", "inner", "leaf"]`.
- **Guaranteed — Nodes queued during a flush run in the same flush.** The
  flush loop runs until the shared queue is empty, including nodes appended
  by a node that is currently running (for example an effect that writes
  another signal). Anchor: `src/scheduler.ts` `flushGlobal`
  (`while (notifyIndex < queuedLength)` over a queue that grows during the
  loop); `tests/scheduler-t16.spec.ts` "flush executes an entire cascading
  chain in the same call". Public-API proof:
  `tests/scheduler-ownership-conformance.spec.ts` "an effect writing
  another signal during a flush runs its downstream effect in the same
  flush".
- **Guaranteed — A nested `flush()` continues the same queue.** Calling
  `flush()` from inside a running node does not start an independent queue:
  the nested call drains the pending remainder of the shared queue, nodes
  appended after the nested call returns but before the outer call resumes
  still run before the outer call returns, and no queued node runs twice.
  Anchor: `src/scheduler.ts` `flushGlobal` shared `notifyIndex`/
  `queuedLength` state; `tests/scheduler-t16.spec.ts` "nested flush
  preserves pending and newly queued node order" and "a drained nested
  flush may append another node before returning".
- **Guaranteed — A throw during a flush drops and re-arms the remainder,
  with no automatic re-run.** If a queued node's run throws out of
  `flush()`, the nodes still queued behind it do not run in that flush and
  are not run by any automatic retry. Their queue slots are dropped and
  their watching/recurred flags are restored, so each one runs again only
  if it is queued again by a later write or scheduling step. (An effect
  error handled by an installed `catchError` hook does not throw out of the
  flush; unhandled effect errors are rethrown and follow this rule.)
  Anchor: `src/scheduler.ts` `flushGlobal` `finally` block (drains the
  remaining queue, sets `FlagWatching | FlagRecursed` on each dropped node,
  resets both indices); `tests/scheduler-t16.spec.ts` "throwing flush
  restores pending nodes so they can be queued again" (a second bare
  `flush()` runs nothing; re-scheduling the node runs it once);
  `src/effect.ts` `EffectNode.run` with `src/context.ts` `reportError`
  for the handled/unhandled distinction.

## Batching

- **Guaranteed — Writes in one task coalesce into one microtask flush.**
  Every write schedules the same deferred flush; until that microtask (or
  an explicit `flush()`) runs, effects do not re-run, and each affected
  effect then runs once observing the final values.
  Anchor: `src/scheduler.ts` `scheduleFlush`/`scheduleGlobalFlush`
  (single `isFlushScheduled` flag around one `queueMicrotask`) and its
  doc comment ("coalescing every write in this task into one run of each
  subscriber"); `tests/owner.spec.ts` "writes stay async until flush
  drains them" (two writes, values `[0]`, then `[0, 2]` after `flush()`).
  Public-API proof: `tests/scheduler-ownership-conformance.spec.ts`
  "two writes in one task produce one effect run after the microtask".
- **Guaranteed — Values are synchronous, effects are deferred.** Reading a
  signal immediately after writing it returns the newly written value,
  while effects that read it re-run only at the next flush (microtask or
  explicit `flush()`). A computed read before the flush likewise settles
  against the new value on demand.
  Anchor: `src/signal.ts` `SignalNode.write` (stores `pendingValue` and
  notifies before deferring via `scheduleFlush`) and `SignalNode.read`/
  `update`; `src/scheduler.ts` `scheduleFlush` doc comment ("Signal
  values still update synchronously on write").
- **Guaranteed — `flush()` forces a synchronous drain.** Calling the
  exported `flush()` runs every queued subscriber before it returns, so
  splitting writes with `flush()` produces one effect run per segment
  instead of one coalesced run. A microtask flush left over after an
  explicit `flush()` finds an empty queue and re-runs nothing.
  Anchor: `src/scheduler.ts` `flush` ("Runs every queued subscriber now,
  including those queued while it runs"); `tests/effect.spec.ts` "a
  microtask flush left over after flush is a no-op (P03)".
  Public-API proof: `tests/scheduler-ownership-conformance.spec.ts`
  "flush in the middle of a task splits coalesced writes into two runs".
- **Guaranteed — There is no `batch()` API.** Batching is not a separate
  public primitive: the exported API in `src/index.ts` contains no
  `batch` export. Coalescing is obtained by writing several signals in
  one task (one microtask flush) or by bracketing writes with `flush()`
  calls. Anchor: `src/index.ts` export list; no `batch` symbol exists in
  `src/` or `tests/`.

## Diamonds and cycles

- **Guaranteed — Pull-based settling via `checkDirty`.** A write only
  marks transitive subscribers pending and queues the watching ones; a
  pending computed is settled on demand: when a queued effect (or a read)
  reaches it, `checkDirty` settles its pending dependencies first and
  recomputes it only if one of them actually changed.
  Anchor: `src/graph.ts` `propagate` (marks `FlagPending`, queues
  watching subscribers) and `checkDirty` (settles pending deps, calls
  `update` only for changed branches); `src/effect.ts`
  `EffectNode.runIfDirty` and `src/computed.ts` `ComputedNode.read`
  both gate recomputation on `checkDirty`.
- **Guaranteed — Glitch-free diamonds.** In a diamond
  (source → two branches → a shared descendant → an effect), one flush
  evaluates the shared descendant once, runs the effect once, and the
  descendant never observes a mix of an old branch value with a new one:
  both branches settle from the same source value before the descendant
  recomputes. Anchor: `src/graph.ts` `propagate` marks the shared
  descendant pending once, `checkDirty` settles both branches before
  `update`; `tests/graph-propagate-t13.spec.ts` "a diamond marks shared
  descendants once and retains inner-write flags". Public-API proof:
  `tests/scheduler-ownership-conformance.spec.ts` "a diamond settles
  each branch once per flush and never mixes old and new values".
- **Guaranteed (development only) — Cycle warning.** In non-production
  builds, a dependency cycle reached while settling a computed warns
  with a `[rezejs] Cycle detected in computed dependencies` message and
  the check terminates instead of descending forever; reading a computed
  while it is being evaluated likewise warns and returns the stale
  value. Anchor: `src/graph.ts` `checkDirty` (the
  `process.env.NODE_ENV !== "production"` `isOnCheckPath` guard) and
  `src/computed.ts` `ComputedNode.read` (`FlagRecursedCheck` warning);
  `tests/cycle.spec.ts` "a cycle formed by a dynamic dependency warns
  and terminates instead of hanging" and "shared dependencies reached
  through several paths are not reported".
- **Unspecified — Cycles in production.** Cyclic computed graphs are
  unsupported. The cycle detection above is development-only and is
  removed from production bundles, where a cyclic graph does not
  terminate; any other production behavior of a cyclic graph (including
  which value, if any, a read returns) is undefined and must not be
  relied on. Anchor: `src/graph.ts` warning text ("in production this
  does not terminate"); `tests/treeShaking.node.spec.ts` "production
  bundles drop dev-only cycle detection" (production output contains no
  `Cycle detected`/`isOnCheckPath`).

## Disposal and ownership

- **Guaranteed — Newest-first disposal.** Disposing an owner disposes
  the nodes and cleanups it owns newest first (reverse creation order),
  and an effect's owned children are disposed before the effect's own
  cleanup runs. Anchor: `src/graph.ts` `disposeChildren` ("Unlinks the
  owned nodes of `sub`, newest first") and
  `disposeAllDepsInReverse`; `src/effect.ts` `EffectNode.runIfDirty`
  disposes children before running the cleanup; `tests/owner.spec.ts`
  "root dispose runs cleanups and effect teardowns in reverse creation
  order" expects `["c2", "e", "c1"]`; `tests/effect.spec.ts` "computed
  unwatched: child effect cleanups run in reverse creation (LIFO)".
- **Guaranteed — Cleanup runs before re-run, and exactly once.** The
  function an effect returns runs before the effect's next run and on
  disposal, whichever comes first. Each returned cleanup runs at most
  once: it is cleared before it is invoked, so a re-entrant disposal
  from inside a cleanup, or a second disposal afterwards, does not run
  it again. If a cleanup disposes its own effect during a re-run, the
  pending re-run does not execute the effect body. Anchor:
  `src/effect.ts` `runCleanup` (clears `node.cleanup` before invoking),
  `EffectNode.runIfDirty` (cleanup before `fn`, early return when the
  node was disposed by its cleanup), `EffectNode.dispose`;
  `tests/effect.spec.ts` "cleanup order on outer re-run: inner before
  outer, before new run".
- **Guaranteed — Detached root.** `root` runs its function in an owner
  detached from the current owner: everything created inside lives
  until the returned `dispose` is called, unaffected by the disposal of
  the owner that was current when `root` was called, while still seeing
  that owner's context. Anchor: `src/owner.ts` `root` doc comment and
  implementation (the new `RootNode` records the current owner as
  `parent` for context lookup but is not adopted by it outside the
  HTML-scope registry).
- **Guaranteed — A disposed queued node never runs.** If an effect is
  queued for a flush and is disposed (directly, through its owner, or
  through an enclosing `effectScope`) before its queue slot is reached —
  including disposal by an earlier effect in the same flush — its body
  does not run for that queued slot, its cleanups still run exactly
  once (newest first), and nodes queued behind it still run.
  Anchor: `src/graph.ts` `disposeNode` (clears flags to `FlagNone` and
  unlinks every dep, so `src/effect.ts` `EffectNode.runIfDirty` finds
  neither dirty/pending flags nor deps and returns without running);
  `src/scheduler.ts` `flushGlobal` still visits the slot and calls the
  now-inert `run`. Public-API proof:
  `tests/scheduler-ownership-conformance.spec.ts` "an effect disposed
  by an earlier effect in the same flush never runs" and "disposing a
  scope with a queued unflushed effect prevents the run".
- **Guaranteed — Double disposal is a no-op.** Calling any returned
  disposer (`effect`, `effectScope`, `root`, `boundary`) a second time
  runs no cleanup again and changes no observable state. Anchor:
  `src/graph.ts` `disposeNode` is idempotent once deps and subs are
  unlinked; `src/effect.ts` `runCleanup` clears the cleanup before
  invoking it, so `EffectNode.dispose` finds nothing on the second
  call. Public-API proof: `tests/scheduler-ownership-conformance.spec.ts`
  "an effect disposed by an earlier effect in the same flush never
  runs" (repeated victim disposal) and the nested-scope tests (repeated
  scope disposal).
- **Guaranteed — Scope nesting and sibling isolation.** Disposing an
  inner `effectScope` disposes only what that scope owns; the outer
  scope and sibling scopes keep working. Disposing the outer scope
  cascades into everything it still owns, newest first. Anchor:
  `src/effectScope.ts` (each scope is a node adopted by the current
  owner; disposal goes through `src/graph.ts` `disposeNode`);
  `tests/effectScope.spec.ts` "scope as intermediate parent: cleanup
  order respects nesting". Public-API proof:
  `tests/scheduler-ownership-conformance.spec.ts` "disposing an inner
  scope leaves the outer scope and sibling scopes alive" and "disposing
  an outer scope cascades to its contents newest first".
- **Unspecified — Disposal of the currently running node, mid-run.**
  An effect or scope disposing itself (or an ancestor) from inside its
  own currently executing body, beyond the cleanup rule above, has no
  specified completion behavior for the remainder of that body: the
  body may finish executing statements after the disposal call, and
  callers must not rely on whether work it creates afterwards is
  tracked or owned. The guaranteed cases are disposal of a queued,
  not-yet-running node and disposal from inside a cleanup. Anchor:
  `src/effect.ts` `runIfDirty` guards only the cleanup-then-rerun path
  (`if (!this.flags) return`); no equivalent guard exists around an
  arbitrary in-body disposal.

## Async cancellation

- **Guaranteed — `asyncComputed` supersede and disposal drop settlements.**
  Each run has a generation. Starting a newer run and disposing the
  computation both advance the generation, and a promise settlement
  whose generation is no longer current — whether superseded or
  post-disposal — is dropped and never overwrites `value()`, `error()`,
  or `isPending()`. Anchor: `src/asyncComputed.ts`
  `AsyncComputedNode.start` (`++this.generation`), `dispose`
  (`++this.generation`), and the settlement handlers
  (`if (generation === this.generation)`); `tests/async.spec.ts` "a
  superseded run never overwrites the latest one" and "disposing the
  owner drops a pending settlement and stops re-running".
- **Guaranteed — `AbortController` on supersede and on dispose.** The
  previous run's controller is aborted when a newer run starts and when
  the computation is disposed. A run's `abortSignal()` is therefore
  aborted once either happens; if `abortSignal()` is first requested
  after the run was already aborted (superseded or disposed), the
  returned signal is already aborted. Reads through a superseded or
  disposed run's `get` are untracked. Anchor: `src/asyncComputed.ts`
  `AsyncRun.abort`/`abortSignal` (lazy controller, immediately aborted
  when `aborted` is already set), `AsyncComputedNode.start`
  (`this.current?.abort()`), `dispose` (same), and `AsyncRun.get`
  (tracks only while generation and flags are current); the
  `AsyncContext` doc comments state the same contract.
- **Guaranteed — `action` has no cancellation.** An `action` call is
  not cancellable: it exposes no abort signal, registers no owner, and
  disposing the owner (root, scope, or effect) that was current when
  the action was called does not cancel, roll back early, or otherwise
  interrupt an in-flight call. Its store writes still commit on success
  and roll back on failure whenever the call settles. Anchor:
  `src/action.ts` contains no `AbortController`, owner adoption, or
  disposal hook; `ActionState.call` settles solely from the wrapped
  function's own resolution or rejection.

## Equality

- **Guaranteed — Default equality is `Object.is`.** With no `equals`
  option, a signal write notifies subscribers only when the new value
  differs from the latest written value by `Object.is`: `NaN` equals
  `NaN`, and `+0` and `-0` differ. Anchor: `src/signal.ts`
  `signal` (`options?.equals ?? Object.is`), `differs` (inlined
  `Object.is`), `SignalNode.write`; `tests/signal.spec.ts` "default
  equality is Object.is: +0 and -0 notify each other (P06)".
- **Guaranteed — `equals: false` notifies on every write.** With
  `equals: false`, every write notifies, even writing the identical
  reference, so in-place mutation followed by writing the same
  reference still propagates. Anchor: `src/signal.ts` `differs`
  (`equals === false` always differs); `tests/signal.spec.ts`
  "equals: false notifies on every write, even with the same value".
- **Guaranteed — Custom `equals` is compared against the latest written
  value, and may be consulted twice per settled write.** A custom
  `equals(prev, next)` is first consulted at write time with `prev`
  equal to the latest written (pending) value — the same value an
  updater function receives. If the write notifies and is later
  settled, it may be consulted a second time with `prev` equal to the
  last settled value. Anchor: `src/signal.ts` `SignalNode.write`
  (`differs(this.equals, this.pendingValue, next)`) and
  `SignalNode.update` (`differs(this.equals, prev, this.pendingValue)`);
  `tests/signal.spec.ts` "custom equals suppresses notification when it
  reports equality".
- **Unspecified — Impure `equals`.** Because a custom `equals` may be
  consulted twice per settled write (and not at all for a write with no
  subscribers to settle), it must be pure: identical inputs must give
  identical results and it must not mutate its arguments or observable
  state. The result of an impure `equals` — which notifications fire,
  and which value a read returns — is unspecified. Anchor: the two
  consultation sites in `src/signal.ts` `write`/`update` above.
- **Guaranteed — Computed results are gated by `Object.is`.** A
  computed notifies its subscribers only when a recomputation produces
  a result that differs from its previous result by `Object.is`; there
  is no custom equality option for `computed`. (Equality gating decides
  notification only: reading the computed still returns the newly
  computed value.) Anchor: `src/computed.ts` `ComputedNode.update`
  (`differs(Object.is, prev, this.value = this.getter(prev))`, with
  `shallowPropagate` only on a differing result in `read`) and the
  `computed` doc comment ("notifies only when the result differs by
  `Object.is`").
