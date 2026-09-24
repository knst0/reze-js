# AGENTS.md

## Self-documented: no commentary

- MUST NOT write `//` comments explaining what/why. Rename, extract fn/type, or tighten signature instead. Only exception: `// SAFETY:` on `unsafe` blocks (invariant + review note).
- `///` rustdoc allowed ONLY on `pub` items whose signature cannot state contract (units, bounds, error cases, complexity). NEVER restate the name.
- Names carry meaning: `pool: &PgPool` not `p: &P`; `deadline: Instant` not `t: u64`; `bytes: Bytes` not `data: Vec<u8>`. Units in names (`timeout_ms`, `cap_bytes`). Booleans read as predicates (`is_sealed`, `has_more`).
- Dead code, commented-out code, `todo!`/`unimplemented!` in delivered code: forbidden. Delete, don't comment.
