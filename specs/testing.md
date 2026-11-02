# Testing

Status: draft. How Reze tests itself. Rules use MUST, NEVER, and SHOULD in the RFC 2119 sense.

## Principle

Tests observe behavior from the outside: a page in a browser, the output or diagnostic of the
compiler, the bytes of a response. No test calls an internal function. The runtime is a private
compiler target, so its internals change freely without a test change; a test changes only when
behavior does.

Unit tests of internal modules are NEVER written.

## Layers

| Layer              | Observes                                                     | Tool                 |
| ------------------ | ------------------------------------------------------------ | -------------------- |
| Browser end-to-end | Fixture applications on the JS server and on the Rust server | Playwright           |
| Compiler fixtures  | Source in, diagnostic or emitted HTML out                    | Snapshots            |
| Render diff        | The JS render and the Rust render of every route             | `reze check`         |
| Size               | Bytes of the first response, the dispatcher, and each chunk  | Size benchmark in CI |

### Browser end-to-end

Each fixture is a small application under `tests/apps/<name>`. A test drives it through
Playwright, in Chromium, Firefox, and WebKit, against both servers:

```ts file=tests/apps/todos/todos.e2e.ts
test("toggles a todo without JS", async ({ page, server }) => {
  await page.context().setJavaScriptEnabled(false);
  await page.goto(server.url("/todos"));
  await page.getByRole("button", { name: "Toggle" }).first().click();
  await expect(page.getByRole("listitem").first()).toHaveClass(/done/);
});
```

`server` is a fixture that runs the application on the JS server or the Rust server; every test
runs on both. A test asserts what a user sees and can do: roles, text, focus, URL, status. It
NEVER asserts markup that only the framework reads, such as markers or `data-rz` attributes.

Every interactivity rule has a test with JS disabled, a test with a slow chunk (the network
throttled until after the click), and a test with a failed chunk.

### Compiler fixtures

Each rule in the specifications that the compiler enforces has a fixture: a source file and the
expected result.

```
tests/compiler/jsx-conditional-expression/
  input.tsx
  diagnostics.snap
tests/compiler/split-text/
  input.tsx
  html.snap
```

A diagnostic fixture snapshots the code, the message, the span, and the fix. An HTML fixture
snapshots the server output, metadata included, so a metadata regression shows in review.
Snapshot updates are reviewed by hand.

### Render diff

`reze check --render-diff` renders every route of every fixture application with its fixture data
through the JS render and the Rust render and compares the bytes. Any difference fails CI.

### Size

The size benchmark builds the fixture applications and records, per page: the compressed first
response, the dispatcher, each chunk, and the metadata share of the HTML. A run that grows past
the recorded snapshot fails; the snapshot changes only in a reviewed commit.

## Running

| Command          | Runs                                      |
| ---------------- | ----------------------------------------- |
| `pnpm test:fast` | Compiler fixtures, end-to-end in Chromium |
| `pnpm test`      | Everything, every browser, both servers   |
| `pnpm bench`     | Size and compile-time benchmarks          |
