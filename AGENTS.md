# AGENTS.md

Reze is a fine-grained reactive UI framework in the Solid family. A Rust compiler turns JSX
into direct DOM operations (client), DOM claiming (hydrate), or string concatenation (server).
Its core rule: **thin runtime, fat compiler.** The runtime is a minimal fast layer (reactive
core + DOM ops); everything statically decidable moves into the compiler — templates, bindings,
constant folding, specialization, islands, feature pruning. We are not building another JS
framework: the compiler may grow, shipped JS must shrink.
With `optimize: true` and `optimize: false`, compiled code must behave the same; `optimize`
only unlocks speculative folds, base compilation already does the maximum static work.

## Layout

| Path                   | What                                                                                  |
| ---------------------- | ------------------------------------------------------------------------------------- |
| `crates/reze_compiler` | oxc-based compiler: analyze → lower (IR holes) → emit. Spec: `SPEC.md` (Russian)      |
| `crates/reze_napi`     | Node binding, built into `packages/compiler`                                          |
| `crates/reze_lsp`      | Language server on top of the compiler's program analysis                             |
| `packages/signals`     | Reactive core (alien-signals port): signal, computed, effect, owners, context, errors |
| `packages/dom`         | DOM + SSR runtime: templates, insert, hydration, islands, control flow, Loading       |
| `packages/reze-js`     | Public entry: re-exports `signals`, `dom`, `dom/flow`, `dom/list`                     |
| `packages/vite-plugin` | Vite plugin: compiles modules, program analysis, feature flags                        |
| `packages/router`      | `@rezejs/router` + `@rezejs/router/vite` (file routes, `virtual:reze-routes`)         |
| `packages/meta`        | `@rezejs/meta`: head tags with SSR collection                                         |
| `packages/devtools`    | `@rezejs/devtools`: owner-tree inspection over the dev-only debug hook                |
| `packages/test-utils`  | `mount`, `fire`, `cleanup`, `tick` for happy-dom tests                                |
| `examples/*`           | Apps built by `benches/bundle-size`                                                   |
| `benches/*`            | `signals` (vitest bench), `dom-rows` (browser), `bundle-size` (gzip log)              |
| `.claude/plans/`       | Implementation plans; mark steps `[x]` as they land                                   |

## Commands

- **Rebuild the napi binding after any Rust change** before running JS tests:
  `pnpm --filter @rezejs/compiler build`.
- Rust: `cargo test --workspace` and `cargo fmt --check`. Snapshots use insta: run
  `INSTA_FORCE_PASS=1 INSTA_UPDATE=new cargo test -p reze-compiler`, review every `.snap.new`
  by hand, then move it over the `.snap`.
- If the diagnostics catalog changes, regenerate its guide with
  `REZE_UPDATE_SKILL=1 cargo test -p reze-compiler --test catalog`. Never format the generated
  `SKILL.md`: the test compares it byte for byte.
- JS tests: run `npx vitest run` inside each package. `pnpm -r test` fails on a workspace cycle.
- Format and lint: `pnpm exec oxfmt <paths>`, then
  `pnpm exec oxlint --type-aware --type-check <paths>`. The known warnings are
  `dom/src/stream.ts` (no-thenable) and `dom/tests/program.spec.ts`. `tsc` errors about
  `__REZE_*` flags are expected, because the plugin defines those flags.
- Bundle size: `node benches/bundle-size/log.mjs`. It rewrites `results/latest.json`; commit
  that file with the change that moved the numbers.
- Run lint, format and tests as one `&&` chain before every commit. Never commit on red.

## Invariants

- **Compiler-first.** New behavior defaults to a compile-time implementation (analysis, codegen,
  specialization). A runtime helper/branch is allowed only when the compiler cannot decide
  statically — state why in the commit/plan. Prefer deleting runtime code via compiler folds
  over adding runtime fast paths.
- **Pay only for what you use.** A new feature must not add bytes to apps that don't use it,
  and SHOULD remove bytes from apps that do (compiler fold preferred over runtime helper):
    - Gate runtime code behind a feature flag, or keep it in a module that only its users import.
    - A flag lives in three places: `crates/reze_compiler/src/features.rs`,
      `packages/dom/src/features.ts` (plus `flags.d.ts`), and the plugin's `Flags` map.
    - Check the result with `bundle-size` and `signals/tests/treeShaking.spec.ts`.
- **Dev-only code** goes behind `process.env.NODE_ENV !== "production"`, written so the
  production branch stays exactly as it was. Devtools hooks follow that pattern:
  `if (dev && debugHook !== undefined) { … return … }`, then the original code.
- **Hot paths** (`signals/src/graph.ts`, `context.ts`, `computed.ts`, `dom.ts` insert and
  reconcile): no new allocations or closures per call. Measure with `pnpm bench`; a regression
  over 3 % needs an A/B rerun before you accept it.
- **Ownership:**
    - `root()`, `RootNode` and `ComputedNode` record `parent`; effects and render bindings are
      adopted as deps of their owner. `parentOwner`/`lookupOwner` walk that chain, which context,
      `Errored` and `Loading` all rely on.
    - A computation created inside a `computed` is owned by that computed and is disposed when it
      re-runs.
- **Compiler changes:**
    - Update `SPEC.md` in the matching section, in Russian.
    - A new runtime component or primitive must be recognized by symbol (§8.0) and, where it
      matters, in `summary/inert.rs`.
    - New diagnostics go into `diagnostic/catalog.rs` with a repair guide.
- Native elements stay native. Router links are plain `<a href>`, and the router intercepts
  clicks on them. Don't add wrapper components where the platform element works.

## Tests

- `packages/signals/tests`: unit tests, including the reactive conformance suite.
- `packages/*/tests/*.spec.tsx`: end-to-end tests compiled by the real Vite plugin, running in
  happy-dom. Compile with `moduleName: "@rezejs/dom"`.
- **Test files are compiled for the client.** Call `renderToString` on components that return
  only other components or runtime values. For real SSR and hydration parity, compile with the
  server target, as `dom/tests/hydrate.spec.ts` and `islands*.spec.ts` do.
- happy-dom quirks:
    - It fetches `<link rel="stylesheet">`, so use another `rel` in tests.
    - It splits text nodes at a raw `>`.
    - `toggleClass(false)` can leave `class=""` behind.
- A browser-facing change (router, islands, hydration) gets one manual pass in Chromium on an
  example: `vite build && vite preview`, driven by Playwright from
  `/opt/node22/lib/node_modules/playwright`.

## Self-documented: no commentary

- MUST NOT write `//` comments explaining what/why, in Rust or TypeScript. Rename, extract a
  fn/type, or tighten the signature instead. Exceptions: `// SAFETY:` on `unsafe` blocks
  (invariant + review note), license headers of ported code, and `// oxlint-disable-next-line`
  with a reason in the same line when unavoidable.
- `///` rustdoc and `/** */` JSDoc are allowed ONLY on public items (`pub`, `export`) whose
  signature cannot state the contract: units, bounds, error cases, complexity, ordering,
  lifetime/ownership. NEVER restate the name.
- Names carry meaning:
    - `pool: &PgPool` not `p: &P`; `deadline: Instant` not `t: u64`; `bytes: Bytes` not
      `data: Vec<u8>`.
    - Put units in names (`timeout_ms`, `cap_bytes`).
    - Booleans read as predicates (`is_sealed`, `has_more`, `isPending`).
- Dead code, commented-out code, `todo!`/`unimplemented!` and `TODO` markers in delivered code
  are forbidden. Delete, don't comment.

## Commits

- One logical change per commit, with the tests, snapshots, `SPEC.md` and bundle-size results
  it needs.
- The subject is imperative, optionally prefixed with a package (`router: …`). The body says
  why.
- Never commit a build output (`dist/`, `.generated/`) or a scratch file.