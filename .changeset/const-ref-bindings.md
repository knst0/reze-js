---
"@rezejs/compiler": patch
---

Stop emitting an assignment fallback for `ref={name}` when `name` is a `const` or an import. Elements now compile to `typeof name === "function" && use(name, el)`, components receive `ref: name` directly, and a `const` bound to a function compiles to a plain `use(name, el)`. Previously the output assigned to the `const`, so bundlers rejected it.
