# reze-js

Reze is a compiler-first UI framework. You write TypeScript and JSX with compiler syntax such as
`signal`, `computed`, `store`, and `action`; the Rust compiler turns it into direct DOM code with
fine-grained bindings, so components run once and only the bindings that read a changed value
update.

```tsx
import { signal } from "reze-js";

export function Counter() {
  let count = signal(0);
  return <button onClick={() => (count += 1)}>Clicked {count} times</button>;
}
```

## Documentation

The documentation site lives in [`docs/`](docs). Start with
[Getting started](https://reze.js.knst.dev/).

## License

This project is licensed under the terms of the [MIT License](LICENSE).
