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
[Getting started](docs/src/routes/installation.mdx).

## Packages

| Package                                               | Purpose                                                                |
| ----------------------------------------------------- | ---------------------------------------------------------------------- |
| [`reze-js`](packages/reze-js)                         | Compiler syntax declarations, `render`, and `hydrate` for applications |
| [`@rezejs/vite-plugin`](packages/vite-plugin)         | Runs the compiler in Vite; file routes and static site generation      |
| [`@rezejs/router`](packages/router)                   | Typed client router                                                    |
| [`@rezejs/compiler`](packages/compiler)               | Native compiler bindings (`compile`, `analyze`)                        |
| [`@rezejs/testing-library`](packages/testing-library) | Test helpers for compiled components                                   |

`@rezejs/signals` and `@rezejs/dom` are the private runtime that compiled code calls.

## Development

| Task                            | Command                          |
| ------------------------------- | -------------------------------- |
| Build packages and the compiler | `pnpm build`                     |
| Run the tests in Chromium       | `pnpm test`                      |
| Run the tests without a browser | `pnpm test:fast`                 |
| Run the compiler tests          | `pnpm test:rust`                 |
| Lint and format                 | `pnpm lint`, `pnpm fmt`          |
| Serve the documentation         | `pnpm --filter @rezejs/docs dev` |

Documentation changes follow [`docs/STYLE.md`](docs/STYLE.md).

## License

This project is licensed under the terms of the [MIT License](LICENSE).
