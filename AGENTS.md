# AGENTS.md

## What Reze is

Reze is a compiler. Applications are TypeScript and JSX plus compiler syntax (`signal`,
`computed`, `action`, `mergeProps`, control-flow tags, async components). The compiler turns them
into direct DOM code that calls a private runtime. There is no uncompiled mode to support.

- **The compiler is the public API.** User-facing reactive syntax exists as type declarations
  only; it has no runtime body. A user-facing capability is compiler syntax with diagnostics
  first, and a runtime helper only for the part the compiler cannot decide.
- **The runtime is a private ABI.** `@rezejs/signals`, the compiler helpers of `@rezejs/dom`,
  and `reze-js/internal/*` exist for compiler output and first-party packages. They version in
  lockstep with the compiler and promise no compatibility: change their shape whenever the
  compiler can emit something better.
- **Analysis crosses module and package boundaries.** Facts about imported modules come from
  on-demand analysis of application modules and from manifests that libraries publish. Facts
  are versioned and checked against the source they describe; missing or stale facts select
  the conservative path.
- **Async is first-party.** Awaiting, pending, and failure are handled by the compiler and
  runtime together, identically for the client, HTML, and hydrate targets.

## Priorities

In order; a lower priority never overrides a higher one.

1. **Correctness.** Behavior is identical across targets and between development and
   production. A missed optimization is safe; an optimization that changes behavior is a bug.
2. **Developer experience.** Code reads as plain TypeScript: fewer wrappers, fewer rules to
   learn, and a diagnostic with a fix for every refused shape. Prefer a compiler rule over a
   runtime API the user has to call.
3. **Runtime performance.** CPU, memory, then bundle size of shipped code. Size is a budget, not
   an absolute: a measured DX gain may cost bytes when the commit body states the trade.

## Compiler

- The compiler exists to improve the runtime. A slower compile is acceptable when it buys
  smaller or faster emitted code; never trade shipped code for compile speed. Compile time still
  matters for dev-server and HMR latency: measure it and optimize the compiler on its own, but a
  small compile-time cost is never a reason to drop an analysis or an optimization.
- Anything decidable statically is decided at compile time: analysis, specialization, constant
  folding, dead-code pruning.
- Prefer removing runtime code over adding runtime fast paths.
- Unknown code is opaque, never guessed: an unanalyzed call, import, or package takes the
  general path.

## Runtime

- Shaped by what the compiler emits, not by hand-written ergonomics.
- **Pay only for what you use.** A feature adds zero bytes to code that doesn't use it: keep it
  tree-shakable, behind a compile-time flag, or in a module only its users import.
- **Hot paths:** no per-call allocations or closures. Prefer flat data, reused structures,
  monomorphic shapes, and early returns over generic abstraction.
- Dev-only code sits behind a compile-time dev check so the production branch is unchanged and
  fully eliminated.
- Use the platform: native elements and APIs over wrappers.

## Measurement

- Measure the runtime CPU, memory, and bundle size of shipped code before and after; don't
  accept a regression without a repeat A/B run.
- Measure compile time and dev-server latency too; a regression there is fixed in the compiler,
  not by giving up emitted-code quality.

## Code is self-documenting

- No `//` comments explaining what/why — rename, extract, or tighten the signature instead.
  Exceptions: `// SAFETY:` on `unsafe`, license headers of ported code,
  lint-disable with an inline reason.
- Doc comments (`///`, `/** */`) only on public items whose signature can't express the
  contract (units, bounds, errors, complexity, ordering, ownership). Never restate the name.
- Names carry meaning: units in names (`timeout_ms`), predicates for booleans (`isPending`).
- No dead code, commented-out code, `todo!`, or `TODO` markers. Delete, don't comment.

## Workflow

- Snapshot updates are reviewed by hand, never accepted blindly.

## Changesets

- NEVER create, edit, or delete `.changeset/*.md`; NEVER run `pnpm changeset` (add) or `pnpm changeset version`. Read-only `pnpm changeset status` is allowed. Versioning and release notes are human-owned.

## Commits

- One logical change per commit, with the tests and docs it needs.
- Imperative subject, optional scope prefix (`router: …`); body says why.
- Never commit build output or scratch files.
