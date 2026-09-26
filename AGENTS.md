# AGENTS.md

## Principles

- **Thin runtime, fat compiler.** Anything decidable statically is done at compile time:
  analysis, codegen, constant folding, specialization, dead-code pruning. Runtime code exists
  only where the compiler cannot decide. The compiler may grow; shipped code must shrink.
- Prefer removing runtime code over adding runtime fast paths.
- Optimizations never change observable behavior.

## Performance

Every change is judged by bundle size, memory, and CPU.

- **Pay only for what you use.** A feature adds zero bytes to code that doesn't use it: keep it
  tree-shakable, behind a compile-time flag, or in a module only its users import.
- **Hot paths:** no per-call allocations or closures. Prefer flat data, reused structures,
  monomorphic shapes, and early returns over generic abstraction.
- Measure before and after; don't accept a regression without a repeat A/B run.
- Dev-only code sits behind a compile-time dev check so the production branch is unchanged and
  fully eliminated.
- Use the platform: native elements and APIs over wrappers.

## Code is self-documenting

- No `//` comments explaining what/why — rename, extract, or tighten the signature instead.
  Exceptions: `// SAFETY:` on `unsafe`, license headers of ported code,
  lint-disable with an inline reason.
- Doc comments (`///`, `/** */`) only on public items whose signature can't express the
  contract (units, bounds, errors, complexity, ordering, ownership). Never restate the name.
- Names carry meaning: units in names (`timeout_ms`), predicates for booleans (`isPending`).
- No dead code, commented-out code, `todo!`, or `TODO` markers. Delete, don't comment.

## Workflow

- Format, lint, and test as one chain before every commit. Never commit on red.
- Snapshot updates are reviewed by hand, never accepted blindly.

## Commits

- One logical change per commit, with the tests and docs it needs.
- Imperative subject, optional scope prefix (`router: …`); body says why.
- Never commit build output or scratch files.
