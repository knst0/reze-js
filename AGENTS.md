# AGENTS.md

## Priority

Ship the smallest correct bundle. Correctness is the constraint: behavior is identical
across the client, island, and HTML targets and between development and production — a
missed optimization is safe, an optimization that changes behavior is a bug. Subject to
that, minimize shipped bytes, then runtime CPU and memory. Move any logic the compiler
can decide into the compiler, even when it costs compile time or compiler complexity.

## Compiler

- Decide statically everything decidable: analysis, specialization, constant folding,
  dead-code pruning.
- Prefer deleting runtime code to adding runtime fast paths.
- Never guess unknown code: an unanalyzed call, import, or package takes the general path.
- Every refused shape gets a diagnostic with a fix. Prefer a compiler rule over a runtime
  API the user has to call.

## Runtime

- The runtime is a private compiler target, not a public API: reshape it freely whenever
  the emitted code gets smaller. No compatibility promise.
- Pay only for what you use: a feature adds zero bytes to code that doesn't use it — keep
  it tree-shakable, behind a compile-time flag, or in a module only its users import.
- Hot paths: no per-call allocations or closures. Prefer flat data, reused structures,
  monomorphic shapes, and early returns.
- Dev-only code sits behind a compile-time dev check so production eliminates it fully.
- Use the platform: native elements and APIs over wrappers.

## Working in this repo

- `crates/reze_compiler` is the compiler core; `packages/compiler` its JS facade.
  `packages/{dom,signals,router,query,vite-plugin,testing-library}` build on it.
- Loop: `pnpm test:fast`; browser matrix: `pnpm test`; Rust: `pnpm test:rust`.
- Measure internal performance before and after with `pnpm bench` (CodSpeed history:
  `pnpm bench:codspeed`); don't accept a regression without a repeat A/B run. Fix compile-time
  regressions in the compiler, never by giving up emitted-code quality.

## Conventions

- No `//` comments explaining what/why — rename, extract, or tighten the signature.
  Exceptions: `// SAFETY:` on `unsafe`, license headers, lint-disable with inline reason.
- Doc comments only on public items whose signature can't express the contract. Never
  restate the name.
- Names carry meaning: units in names (`timeout_ms`), predicates for booleans (`isPending`).
- No dead code, commented-out code, or `TODO` markers. Delete, don't comment.

## Process

- Review snapshot updates by hand, never accept blindly.
- NEVER create, edit, or delete `.changeset/*.md`; NEVER run `pnpm changeset` (add) or
  `pnpm changeset version`. Read-only `pnpm changeset status` is allowed.
- One logical change per commit, with the tests and docs it needs. Imperative subject,
  optional scope prefix (`router: …`); body says why. Never commit build output or scratch.
