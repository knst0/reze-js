# reze-js

Signals-based UI framework with a Rust compiler and thinest runtime. JSX is compiled to direct DOM operations.

```sh
pnpm add reze-js
pnpm add -D @rezejs/vite-plugin vite
```

## Compilation

Constant boolean-attribute expressions and `checked`/`selected` expressions preserve JavaScript
truthiness in both client output and prerendered HTML. Numeric zero, including `-0`,
does not enable them; the string `"0"` does. Text and ordinary attributes retain their
string representation.

Native inline object spreads with compiler-known, unique keys can use direct per-key
updates instead of the generic spread dispatcher. Eager values are captured once;
reactive reads and DOM writes retain their source order in one effect. Aliased or dynamic
sources, duplicate or integer-like keys, events, spread-provided refs/children, and
getter-context-sensitive expressions retain the generic path.

Specialization removes the props object, getter wrappers, and runtime key dispatch.
It can also remove the generic spread helper from bundles that no longer use it.
Individual call sites can grow when that helper is still needed elsewhere.

## License

This project is licensed under the terms of the [MIT License](LICENSE).
