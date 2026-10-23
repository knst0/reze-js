# @rezejs/compiler

## 0.2.0

### Minor Changes

- 37541b7: Allow components in `src/document.tsx`, including imported ones. The compiler follows the document's imports into source files and `reze`-condition packages and inlines their components into the static HTML at build time, with props replaced by the literal values passed where they are used, literal constants folded, and `children` replaced by the markup between their tags. A document the compiler cannot read statically no longer fails: the plugin runs it once on the server, at build time and at the first dev render, and `DOCUMENT_DYNAMIC` becomes an informational diagnostic that names the part that made it run.
- 32f7d67: Render server pages from a static `src/document.tsx` instead of `index.html`. The `ssg`/`ssr` options `template` and `selector` are replaced by `document`; the element that holds `children` is the mount root, and the plugin adds the client script. A leftover `index.html` fails the build with a message to move its head tags into the document. The compiler adds `compileDocument` and the `DOCUMENT_SHAPE`, `DOCUMENT_DYNAMIC` and `DOCUMENT_ROOT` diagnostics. A render that fails before the first byte now answers a `500` page inside the document, and in development shows the Vite error overlay and keeps HMR connected.

### Patch Changes

- 718a372: Stop emitting an assignment fallback for `ref={name}` when `name` is a `const` or an import. Elements now compile to `typeof name === "function" && use(name, el)`, components receive `ref: name` directly, and a `const` bound to a function compiles to a plain `use(name, el)`. Previously the output assigned to the `const`, so bundlers rejected it.
- 1a018a1: Speed up builds by hashing module sources in the native compiler instead of with BigInt arithmetic in JavaScript, and by reading and hashing each imported file once per change instead of on every importer lookup. `@rezejs/compiler` exports the hash as `profileHash`.
- 294df75: Compile and analyze modules on the libuv thread pool during Vite builds, so several modules compile in parallel instead of blocking the main thread one at a time. `@rezejs/compiler` exports the promise-returning `compileAsync` and `analyzeAsync`.

## 0.1.0

### Minor Changes

- 0162996: Initial release.
