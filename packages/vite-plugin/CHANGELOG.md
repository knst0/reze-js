# @rezejs/vite-plugin

## 0.2.0

### Minor Changes

- 8c1e6c6: Leave the `beforeLoad` cascade out of production bundles that never use it. A build defines `__REZE_BEFORE_LOAD__` as `false` when no file in the project mentions `beforeLoad`, and the router drops the cascade from settling and link warming. A dependency that defines `beforeLoad` in such a build fails it with a message to declare that route config in the app's sources. Development and builds without the plugin keep the cascade.
- 37541b7: Allow components in `src/document.tsx`, including imported ones. The compiler follows the document's imports into source files and `reze`-condition packages and inlines their components into the static HTML at build time, with props replaced by the literal values passed where they are used, literal constants folded, and `children` replaced by the markup between their tags. A document the compiler cannot read statically no longer fails: the plugin runs it once on the server, at build time and at the first dev render, and `DOCUMENT_DYNAMIC` becomes an informational diagnostic that names the part that made it run.
- 1bcd787: Render server pages from the file routes alone. `ssg` and `ssr` accept `true` for every default, turn `fileRoutes` on when it is not set, and drop the `entry` option: there is no `src/app.tsx` or server entry module, shared markup lives in the document, and a leftover `entry` fails with a message to remove it. `fileRoutes: false` together with `ssg` or `ssr` is an error. With `fileRoutes` on, `reze()` returns a promise of plugins that Vite's `plugins` array awaits. The server `createContext` factory is gone, along with the `RouterServerContextArgs` and `RouterServerContextFactory` types and the `context` option of server route preparation; a layout's `beforeLoad` supplies those values instead.
- d7b5a63: Fill a new empty route file with a starter component during `vite` dev, so a fresh file renders instead of failing for a missing default export. A page types its props by its route pattern; a file beside a same-named directory becomes a layout that renders `children`. `@rezejs/router/fs` exports the generator as `routeTemplate`. Set `fileRoutes.template: false` to turn it off.
- 316265d: Take a base router `context` per request: `handler(request, { context })` passes it to the routes, each layout's `beforeLoad` extends it, and `preload` and `redirect` read it. Use it for cookies, headers and platform bindings, which `beforeLoad` cannot see. `ssr.devContext(request)` supplies the same value to pages the `vite` dev server renders.
- 32f7d67: Render server pages from a static `src/document.tsx` instead of `index.html`. The `ssg`/`ssr` options `template` and `selector` are replaced by `document`; the element that holds `children` is the mount root, and the plugin adds the client script. A leftover `index.html` fails the build with a message to move its head tags into the document. The compiler adds `compileDocument` and the `DOCUMENT_SHAPE`, `DOCUMENT_DYNAMIC` and `DOCUMENT_ROOT` diagnostics. A render that fails before the first byte now answers a `500` page inside the document, and in development shows the Vite error overlay and keeps HMR connected.

### Patch Changes

- 7c41864: Refresh the route table in every dev environment when a route file is added, renamed, or deleted. Previously only the client environment reloaded `virtual:reze-routes`, so server renders kept the old table and failed with `Internal Server Error` until the dev server restarted.
- 1a018a1: Speed up builds by hashing module sources in the native compiler instead of with BigInt arithmetic in JavaScript, and by reading and hashing each imported file once per change instead of on every importer lookup. `@rezejs/compiler` exports the hash as `profileHash`.
- 294df75: Compile and analyze modules on the libuv thread pool during Vite builds, so several modules compile in parallel instead of blocking the main thread one at a time. `@rezejs/compiler` exports the promise-returning `compileAsync` and `analyzeAsync`.
- e5f2127: Pass the request's query string to server routes. `location.query` and `location.search` in `beforeLoad`, `preload` and `redirect` were empty during SSR.
- 629483a: Declare the root type entry in the top-level `types` field so npm detects the bundled TypeScript declarations.
- Updated dependencies [8c1e6c6]
- Updated dependencies [91a50e8]
- Updated dependencies [718a372]
- Updated dependencies [37541b7]
- Updated dependencies [1bcd787]
- Updated dependencies [1a018a1]
- Updated dependencies [294df75]
- Updated dependencies [d7b5a63]
- Updated dependencies [316265d]
- Updated dependencies [32f7d67]
- Updated dependencies [629483a]
  - @rezejs/router@0.2.0
  - @rezejs/compiler@0.2.0
  - reze-js@0.1.1

## 0.1.0

### Minor Changes

- 0162996: Initial release.

### Patch Changes

- Updated dependencies [0162996]
  - reze-js@0.1.0
  - @rezejs/compiler@0.1.0
  - @rezejs/router@0.1.0
