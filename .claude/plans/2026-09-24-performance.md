# Plan: Performance and bundle-size improvements

## Goal

Close the few measured gaps between Reze and the fastest peers (Solid 1.9, Solid 2.0 RC, Vue Vapor 3.6) and shrink the runtime shipped to typical apps. Measured baseline (2026-09-24, Chromium headless, median of 12, script + forced layout, 1000-row table):

| op | reze | solid 1.9 | solid 2 rc | vue vapor | react 19.3 | react + compiler |
|---|---|---|---|---|---|---|
| create 1k | 29.1 | 30.8 | 31.2 | 31.6 | 34.9 | 45.3 |
| update every 10th | 8.6 | 9.4 | 9.2 | 8.1 | 11.6 | 10.8 |
| swap 2 rows | 3.1 | 2.7 | 2.9 | 2.6 | 39.4 | 38.6 |
| select row | 0.3 | 0.1 | 0.4 | 0.1 | 2.3 | 0.7 |
| remove row | 2.8 | 2.7 | 2.6 | 2.6 | 5.5 | 3.1 |
| append 1k | 41.4 | 41.9 | 40.8 | 40.6 | 48.2 | 51.7 |
| create 10k | 404.9 | 410.1 | 393.4 | 423.2 | 673.7 | 724.5 |
| clear 10k | 36.6 | 41.6 | 44.6 | 43.5 | 61.5 | 61.6 |

Counter app bundle (JS, gzip): reze 3.84 kB (4.14 kB with Vite's modulepreload polyfill), solid 1.9 4.48, solid 2 rc 13.87, vue vapor 17.52, react 68.5.

Done = every step below merged, and on the rows benchmark (step 1) Reze is ≤ the best peer on select, swap and remove, with no regression > 3 % elsewhere; counter bundle ≤ 3.5 kB gzip.

## Context / constraints

- Follow `AGENTS.md`: no explanatory `//` comments in new code; names carry meaning; no dead code, no `todo!`.
- The compiler spec is `crates/reze_compiler/SPEC.md` (Russian). Every compiler behavior change adds or edits the matching SPEC section, in Russian, in the same style (§7.x for JSX semantics, §8 `O<n>` for optimizations, §15 for program analysis).
- Every compiler optimization (SPEC §8 preamble): the applicability condition is checked by symbol resolution, not by name; when in doubt, do not apply; each application emits an `info` diagnostic with its reason (§9.5); each has a differential test (`optimize: true` vs `false`, §12).
- Compiler tests: `crates/reze_compiler/tests/snapshots.rs` (insta, all three targets: client, `server__…`, `hydrate__…`), `tests/compile.rs` (behavioral), `tests/programs.rs` + `tests/programs/<name>/` (whole-program). Runtime e2e tests: `packages/dom/tests/*.spec.ts(x)` through the Vite plugin in happy-dom.
- Runtime helper names map to imports in `crates/reze_compiler/src/emit/mod.rs` (`enum Helper`, both `name()` and the `_$` alias table). Adding a helper = new variant in both tables + export from `packages/dom/src/index.ts` (and it is re-exported by `packages/reze-js/src/index.ts` via `export *`).
- IR lives in `crates/reze_compiler/src/ir.rs` (`Op`, `Bind`, `BindTarget`); lowering in `src/lower/`; client/hydrate emission in `src/emit/template.rs`; server emission in `src/emit/server.rs`.
- Signals core (`packages/signals/src/graph.ts`) is a port of alien-signals and already matches it; do NOT change propagation/`checkDirty` in this plan.
- Build the native compiler before JS tests: `pnpm --filter @rezejs/compiler build`.
- Bundle numbers: `pnpm --filter @rezejs/bench-bundle-size log` (writes `benches/bundle-size/results/latest.json`; commit the updated file with each step that changes size).

## Steps

- [ ] 1. Add a DOM rows benchmark and fix the signals fan-out benchmark
      File(s): new `benches/dom-rows/` (package `@rezejs/bench-dom-rows`: `package.json`, `vite.config.ts`, `index.html`, `src/main.tsx`, `run.mjs`, `results/latest.json`); `benches/signals/src/fanout.bench.ts`; root `package.json` `bench` script.
      Intent: `src/main.tsx` renders a `<tbody>` of 1000 rows with `<For>`, each row `{ id, label: signal }`, a module-level `selected` signal, and row class `selected() === row().id ? "danger" : ""`. It exposes `window.ops` with `create1k, create10k, update10th, swap (rows 1 and 998), select (row 5), remove (index 4), append (1k), clear, flush` (flush = `flushSync`). Labels come from a seeded PRNG (seed 1, Park–Miller `seed = seed * 16807 % 2147483647`) so runs are deterministic. `run.mjs` builds nothing; it serves `dist/` with `node:http`, launches Chromium via `playwright-core` with `executablePath: process.env.CHROMIUM_PATH ?? "/opt/pw-browsers/chromium"` and `--js-flags=--expose-gc`, and per op runs 12 iterations of: clear, `gc()`, setup, then measure `op(); flush(); document.body.offsetHeight` with `performance.now()`, waiting one `requestAnimationFrame` + `setTimeout` between ops. It asserts 1000 `<tr>` after `create1k`, prints median ms per op, compares to `results/latest.json` like `benches/bundle-size/log.mjs` does, and rewrites it. The package script `bench` = `vite build && node run.mjs`. In `fanout.bench.ts`, call `flushSync()` after each `set(next)` so the effects actually run.
      Done when: `pnpm --filter @rezejs/bench-dom-rows bench` prints 8 ops and writes `results/latest.json`; the root `bench` script also runs it; the fan-out bench result reflects 100 effect runs per write (its time rises well above the old 1.9 ms).

- [ ] 2. Disable Vite's modulepreload polyfill by default
      File(s): `packages/vite-plugin/src/index.ts`, `packages/vite-plugin/tests/` (new `config.spec.ts`).
      Intent: add a `config(userConfig)` hook to the returned plugin. If `userConfig.build?.modulePreload` is `undefined`, return `{ build: { modulePreload: { polyfill: false } } }`; otherwise return nothing (user choice wins). Add option `modulePreloadPolyfill?: boolean` (default `false`, rustdoc-style JSDoc stating that `true` restores Vite's default) that, when `true`, makes the hook return nothing.
      Done when: building `examples/counter` produces a JS asset without `relList.supports("modulepreload")`; the test asserts both the default and the opt-out; bundle-size results show ≈ −260 B gzip on every example.

- [ ] 3. Add the `selector()` primitive
      File(s): new `packages/signals/src/selector.ts`; `packages/signals/src/index.ts`; `packages/dom/src/index.ts` (re-export); new `packages/signals/tests/selector.spec.ts`; `packages/signals/tests/treeShaking.spec.ts`.
      Intent: `export function selector<K>(source: Getter<K>, equals?: (a: K, b: K) => boolean): (key: K) => boolean`. Semantics: `isSelected(key)` returns `equals(key, source())` (default `Object.is`) and, when called inside a tracking context, subscribes the caller ONLY to changes of that key's result. Implementation: one internal effect-like node (owned by the current owner, created with `enterOwner` like `effect`) reads `source()`; on change it looks up the previous and the new key in a `Map<K, KeyNode>` and notifies only those two `KeyNode`s. A `KeyNode` is a `ReactiveNode` with `FlagMutable`, `subs/subsTail`, and an `update()` returning whether its boolean flipped; notify via `propagate(node.subs, effectDepth !== 0)` + `scheduleFlush()` exactly as `signalSet` does. `isSelected(key)` gets or creates the `KeyNode`, calls `track(keyNode)`, returns the boolean. `KeyNode.unwatched()` deletes the key from the map (no leak when rows go away). With `equals` provided, fall back to scanning all live `KeyNode`s on each change (documented O(n) in the JSDoc). Nothing in `graph.ts` changes.
      Done when: tests prove (a) a change from key 1 → 2 re-runs exactly the two dependent effects out of 1000; (b) keys with no subscribers are removed from the map after their effects are disposed; (c) works inside `batch` and with `flushSync`; (d) `treeShaking.spec.ts` shows `selector` code absent from a bundle that does not import it. Rows bench `select` ≤ 0.15 ms when the bench row uses `isSelected(row().id)`.

- [ ] 4. Compiler: automatic selector inside `<For>` (like Vue Vapor)
      File(s): new `crates/reze_compiler/src/lower/selector.rs` (registered in `src/lower/mod.rs`); `src/emit/mod.rs` (`Helper::Selector` → `selector`); `SPEC.md` new §8 `O6. Автоматический selector в <For>`; snapshots + `tests/compile.rs`; `packages/dom/tests/list.spec.tsx`.
      Intent: under `optimize`, inside the children callback of a runtime `For` (resolved by symbol per §8.0 — reuse the check `lower/component.rs` does for `name == "For"`, but by symbol), a binding or insert expression of the exact shape `S() === K` or `K === S()` (also `!==`, rewritten to `!_sel$(K)`) is rewritten to `_sel$(K)` when: `S` resolves to a `signal` getter or `computed` declared OUTSIDE the For callback; `K` is built only from the callback's parameters (`row()`, `row().a.b`, `index()`), literals and member access; the expression is not inside a nested function. Hoist one `const _sel$N = selector(S)` per distinct `S` into the component body, emitted immediately before the statement containing the `<For>` element, so it is owned by the component. Emit `info` diagnostic `AUTO_SELECTOR` with the rewritten span. Add the code to the diagnostic catalog (`src/diagnostic/catalog.rs`) and to `packages/compiler/skills/compiler-diagnostics` (catalog test enforces this).
      Done when: snapshot of `<For each={rows()}>{(row) => <tr class={selected() === row().id ? "danger" : ""}/>}</For>` shows `_sel$0(row().id)` for client and hydrate targets and is unchanged for server; with `optimize: false` output is unchanged; the DOM e2e test shows identical DOM in both modes after selecting several rows; rows bench `select` ≤ 0.15 ms with the bench's original (unmodified) row code.

- [ ] 5. Compiler: object `class` with static keys → `classList.toggle`
      File(s): `crates/reze_compiler/src/lower/attribute.rs` (`fn class`); `src/ir.rs` (`BindTarget::ClassToggle(&'a str)`); `src/emit/template.rs`; `src/emit/mod.rs` (no new helper — emit `el.classList.toggle(name, v)` inline); `SPEC.md` §7.3; snapshots; `packages/dom/tests/dom.spec.tsx`.
      Intent: today `class={{ negative: count() < 0 }}` becomes a bind that builds a new object each run, so `v !== p[i]` is always true and `className` → `classTokens` allocates and runs a regex every update. New rule in `fn class`, before the generic dynamic path: if every class source is either a string literal or an object literal whose properties are all non-computed identifier/string keys (no spread, no methods, no getters), then (a) all string sources and all object keys whose value is a literal (after §7.10 folding) are folded into the template's static `class="…"` exactly as the static path does; (b) every remaining key is split on ASCII whitespace into tokens, and each token becomes a `Bind { target: BindTarget::ClassToggle(token), value: Value::Expr(!!(value)) }` (or `Op::Set` when not reactive). If any token appears both in the static part and as a toggle, or twice as a toggle, fall back to the existing `className` path. Emission: the merged bind compares the boolean with its prev slot as usual and runs `node.classList.toggle("token", v)`. Server target is unchanged (still `ssrClass`, same HTML). Hydrate: the server-rendered `class` attribute already contains the right tokens; the first bind run toggles to the same state (no mismatch).
      Done when: counter snapshot shows `_el$2.classList.toggle("negative", _v$)` with a boolean `_v$`, and no `className` import; DOM tests cover: static + dynamic keys merged, multi-token key (`{"a b": on()}`), fallback on duplicate token, hydrate of a toggled class; counter bundle no longer contains `classTokens`.

- [ ] 6. Compiler: string-only `class` → attribute setter
      File(s): `src/lower/attribute.rs`; new `src/lower/types.rs` (shared "static type" classifier, also used by step 7); `src/emit/mod.rs` (`Helper::SetAttribute` already exists — reuse it); `SPEC.md` §7.3; snapshots.
      Intent: add `pub fn static_kind(e: &Expression, facts: &Facts) -> Option<StaticKind>` with `enum StaticKind { Number, String }`, returning `Some` only when provable: numeric literals, unary `-`/`+`/`~`, binary `- * / % ** | & ^ << >> >>>`, `+` where both sides are `Number`; string literals, template literals, `+` where either side is `String`; conditional `c ? a : b` where both branches have the same kind; `String(x)`, `x.toString()`, `x.toFixed(n)`, `.join(...)` → `String`; `Number(x)`, `.length` → `Number`; reads of O3-folded constants whose initializer has a kind. Everything else → `None`. In `fn class`, when there is exactly one class source, it is an expression, and `static_kind` is `String`, emit the bind as `setAttribute(el, "class", v)` instead of `className`.
      Done when: `class={selected() === row().id ? "danger" : ""}` compiles to `setAttribute(_el$, "class", …)`; unit tests in `tests/compile.rs` cover each `static_kind` rule including negative cases (`a + b` with unknown operands → `None`, `props.x` → `None`, boolean comparisons → `None`); rows bench shows no `className`/`classTokens` in its bundle.

- [ ] 7. Compiler: text-only inserts → text node + `setText`
      File(s): `src/lower/children.rs`; `src/ir.rs` (new `Op::Text { node: NodeId, parts: Vec<TextPart> }` or a `BindTarget::Text`); `src/emit/template.rs`; `src/emit/server.rs`; `src/emit/mod.rs` (`Helper::SetText` → `setText`); `packages/dom/src/dom.ts` + `index.ts` (`export function setText(node: Text, value: string): void` — assigns `node.data` only when different); `SPEC.md` §7.5; snapshots for all three targets; `packages/dom/tests/hydrate.spec.ts`.
      Intent: in a native element's children, a maximal run of adjacent static text and dynamic expressions whose `static_kind` (step 6) is `Some` becomes ONE text node in the template (its static text, or a single space when the run starts empty), located like any other template node, plus one bind `setText(t, "static" + String(v) + …)` merged into the element's bind (O2). Runs containing any expression with `static_kind == None` keep today's `insert` path. Server: emit the concatenated, HTML-escaped text with no `<!--[-->…<!--]-->` markers. Hydrate: claim the text node positionally (`claimChild` counts the run as one child). Safety rule for server/hydrate targets only: use the text path only if the run has non-empty static text or every dynamic part is `Number` (an empty string would produce no text node in parsed HTML and break claiming); otherwise keep `insert`.
      Done when: counter snapshot shows `<p>doubled: </p>` text node + `setText`, and `insert` is only used for `<Show>`; client/hydrate/server snapshots updated; hydrate e2e test covers a numeric run, a string run with static prefix, and the fallback for a pure string expression; counter bundle drops `normalizeIncomingArray` if no other insert needs it (check with `vite build --minify false`).

- [ ] 8. Program analysis: fold constant component props
      File(s): `crates/reze_compiler/src/link.rs`; `src/summary/mod.rs` (record JSX call sites per component with literal props); `SPEC.md` new §15.x `Свёртка константных props`; new `tests/programs/props_constant_fold/` + snapshot; `packages/vite-plugin/tests/program.spec.ts`.
      Intent: in whole-program mode (`vite build`), for a program component `C` and prop key `k`: if every reference to `C` in the program is a JSX tag (no other value use: not exported past the program boundary, not passed as a value, not used with `Dynamic`), no call site uses a spread, and every call site passes `k` as the SAME literal (per §7.10) — then every `props.k` read in `C` (after §15.7 destructuring rewrite) is replaced by that literal, so it folds into the template like O3. Absent at some site → do not fold. Emit `info` `PROP_FOLDED` with the key, literal and the call sites; add to the catalog + skills file.
      Done when: `examples/counter` build output contains `−1` and `+1` as static template text and no `insert(..., () => props.step, ...)`; the program test shows the fold refused (with reason) when one call site passes a different value, a spread, or an identifier.

- [ ] 9. Lower `<Show>` without function children to a conditional
      File(s): `crates/reze_compiler/src/lower/component.rs` and `src/lower/children.rs` (reuse the existing conditional lowering, SPEC §7.5 "Условие"); `packages/dom/src/flow.ts`; `SPEC.md` §7.8; snapshots; `packages/dom/tests/flow.spec.tsx`.
      Intent: compiler — when the element is the runtime `Show` (by symbol, §8.0), has a `when` attribute, has no spread, its children are not a single function expression, and `fallback` is absent or a JSX/literal expression: lower it exactly as the JSX conditional `when ? children : fallback` (one `Op::Memo` of `!!(when)` plus `insert`). Runtime — in `Show`, create the `when` memo only when `children` is a function with `length > 0`; otherwise pass nothing and use `shown = computed(() => !!props.when)` directly, so the runtime path uses 2 computeds instead of 3.
      Done when: counter snapshot has no `Show` import and a `memo(() => !!(count() >= 10))`; `flow.spec.tsx` passes unchanged plus a test that a function child still receives a working getter; a test counts graph nodes (via disposing and counting `onCleanup` calls or a dev hook) showing 2 computeds for the non-function case.

- [ ] 10. Single-pass keyed list reconciliation
      File(s): `packages/dom/src/list.ts`; `packages/dom/src/dom.ts` (`insertExpression`); new `packages/dom/src/reconcile.ts` (LIS move planner); `packages/dom/tests/list.spec.tsx`, `packages/dom/tests/reconcile.spec.ts`.
      Intent: today `For` diffs rows by key, returns a plain array, and `insertExpression` → `normalizeIncomingArray` → `reconcileArrays` diffs the DOM again with a second `Map`. Change `For` to return (from its computed) an object `ListOutput { nodes: Node[]; previousIndex: Int32Array; removed: Node[] }` branded with a well-known symbol property `$$list`, ONLY when every row value is a single `Node` (check `nodeType`); otherwise return the plain array exactly as today. `previousIndex[i]` = the row's index in the previous `nodes` or `-1` for a new row — `For` already knows this from its `byKey` pass. In `insertExpression`, before the `Array.isArray` branch: if `value.$$list` is set and `current` is an array of the previous list's nodes, call `applyListMoves(parent, current, value, marker)` from `reconcile.ts`: remove `removed` nodes, compute the longest increasing subsequence of `previousIndex` (O(n log n), ignoring −1), and walking from the end insert new nodes and move nodes not in the LIS before the next node (or the marker/`after` node). Then `current = value.nodes`. `dom.ts` must not import `list.ts` (duck-typed check on the property) so apps without `For` do not pay for it.
      Done when: a fuzz test (seeded, 500 iterations, random insert/remove/move of up to 50 keyed rows) asserts DOM order equals data order and that kept rows keep their nodes; the existing list tests pass; rows bench swap, remove and append each improve or stay within 3 %, and swap ≤ 2.7 ms.

- [ ] 11. Measure-then-decide: row creation cost
      File(s): `packages/dom/src/list.ts`, `packages/signals/src/owner.ts`, `packages/signals/src/signal.ts` (only if a gain is proven); `benches/signals/src/` (new `create.bench.ts`).
      Intent: add a bench creating 10k `signal + computed + effect` and 10k `For` rows. Try, one at a time, keeping only changes that give ≥ 5 % on `create 1k`/`create 10k` in the rows bench without regressing other ops: (a) in `For`, skip `signal(item)` for keyed rows until `row.update` sees a different item (start with a plain getter; switch to a signal on first change — requires the getter to be a stable function that reads a nullable signal); (b) replace `.bind(node)` in `signal()` with closures and keep `isSignal` working via a module-level `WeakSet` of getters — keep only if faster; (c) use a lighter per-row owner than `RootNode` if profiling shows `root()` overhead. Record results (kept or rejected, with numbers) in the commit message.
      Done when: the new bench exists; each experiment is either merged with numbers or reverted; no public API changes.

## Verification

- `cargo test -p reze-compiler` (snapshots reviewed with `cargo insta review`, no unexpected changes).
- `pnpm --filter @rezejs/compiler build && pnpm -r test`.
- `pnpm lint && pnpm fmt --check` (or the repo's fmt check equivalent) and `cargo fmt --check`.
- `pnpm bench` — compare `benches/*/results/latest.json` with the baseline table in Goal; targets: select ≤ 0.15 ms, swap ≤ 2.7 ms, counter ≤ 3.5 kB gzip.

## Out of scope

- Changing the signal graph algorithm (`graph.ts`, `checkDirty`, `propagate`).
- Changing the public shape of `signal()` (tuple of getter/setter) or `For`'s props.
- Type-checker-based inference (no TypeScript type information in the compiler); `static_kind` stays syntactic.
- Event delegation changes, SSR streaming performance, HMR performance.
- Adding peer frameworks (Solid, Vue, React) as repo dependencies; peer comparisons stay ad-hoc.
