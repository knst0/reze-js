# AGENTS.md

## Build

- Use `cargo blaze build|check|test` when installed; otherwise plain `cargo`. Same flags, output and `target/`.
- `cargo blaze status` explains what the daemon holds and why a build fell back to cargo.
- blaze misses some borrow errors: plain `cargo check` is the final word.
- Keep rustup's `cargo`/`rustc` first in `PATH`; blaze needs `rust-src`.

## CI

- CI is local: `cargo xtask ci`. Run it before every commit.
- Never add `.github/workflows/*` or any hosted CI.

## Commits

- One logical change per commit.
- Imperative subject, optional scope prefix (`router: …`), no body.
- Commit to `main`; feature branches only for WIP.
- Always commit `Cargo.lock`.

## Tests

- Write and run tests to verify changes, then delete them before committing.
- Commit tests only when the user explicitly asks. No answer means no tests.

## Code

- No `//` comments or doc comments. Rename, extract or tighten the signature instead.
  Exceptions: `// SAFETY:` on `unsafe`, license headers of ported code.
- Put units in names (`timeout_ms`); name booleans as predicates (`is_pending`).
- Suppress lints only with `#[expect(lint, reason = "...")]`. `#[allow]` is banned.
- No dead code, commented-out code, `todo!` or `TODO`.
- No `unwrap`/`expect`/`panic!` outside tests.
- No `static`/`OnceLock` state, `Rc`/`RefCell`, `async-trait`, `lazy_static`/`once_cell`, or locks held across `.await`.

## External changes

- Worktree changes you didn't make belong to the user. Never edit, revert, reformat or absorb them.
- If they overlap your task, stop and ask.
