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

## License

This project is licensed under the terms of the [MIT License](LICENSE).
