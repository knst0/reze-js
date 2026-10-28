# @rezejs/vite-plugin

## 0.3.0

### Minor Changes

- 5aab4af: Let islands adopt the server DOM when the compiler proves the view is a static element tree with value bindings only. The island claims the existing roots instead of rendering a second copy and swapping it in, so scroll positions, focus, form values and playing media inside it survive the load and the nodes keep their identity. Components, control flow, async components, dynamic tags, `innerHTML`/`textContent` bindings and islands with rendered children keep the replacing path; the browser validates every claimed root and falls back if the server markup does not match.
- 0e0d10d: Keep unchanged route levels across navigation. The server marks every layout and page with a key that carries its id, params, data and pending async work; a swap replaces only from the first level whose key changes, so sidebar scroll, open menus, playing video and island state in a layout survive. An element with `persist="key"` moves from the current page into the matching element of the next one even when its level is replaced. The swap runs inside `document.startViewTransition` when the browser provides it and the user does not prefer reduced motion.
- 981d711: Reject module state written during a request render. The compiler reports `MODULE_STATE_SHARED` as an error on the html target when a component or a route `preload`, `beforeLoad`, `redirect`, `loader`, or `action` writes a module-level `let` or `var`, because that module is loaded once per process and every request shares the value. Values declared with `signal`, `store`, `selector`, `createContext`, `effect`, or `provideContext` keep their per-request copies, and writes inside event handlers stay legal.
- 2727fa4: Prerender up to the first async boundary and stream the rest. `ssg: { ppr: true }` writes a shell with the layouts and fallbacks plus a resume script; the browser fetches the settled parts from the same route with `x-reze-resume: 1`, and the handler streams only the patches below the boundary. `cache(load, { ttl, tags })` keys async data by its JSON arguments on the server, and the server entry exports `revalidateTag(tag)` to drop every entry with that tag.
- b8a0cea: Choose how a route renders. `export const route = { render: "static" | "server" | "client" }` moves the choice from the build to the route: with `ssg`, `static` leaves become files and `server` leaves are rejected unless `ssr` is built too; `client` renders every layout above the route on the server and mounts a client router for the route subtree, so a static marketing section and a browser-rendered account area live in one application.
- e08592c: Add server actions. `server(schema, run)` with a schema from `reze-js/schema` becomes a typed POST endpoint: `.call(input)` posts JSON and resolves to the handler's result, and `.endpoint` works as a `<form method="post">` action without JavaScript, redirecting to `_return` or back with `_action_error`. The compiler rejects a `server` call without an input schema, because every action is a public endpoint; the handler still authorizes the caller. The server entry exposes the endpoint in both the production handler and the dev server.
- 4ff4ef8: Render client-only pages from `src/document.tsx` instead of `index.html`. Without `ssg` or `ssr`, the plugin compiles the document into the HTML entry, serves it from the dev server for every path that accepts `text/html`, and reloads the page when the document or a component it inlines changes. The document must be static, because the plugin compiles it into the entry instead of running it on a server. A leftover `index.html` next to the document fails the build with a message to move its tags into the document. A project with an explicit bundler input or without `src/document.tsx` keeps Vite's `index.html`.

### Patch Changes

- 67b1717: Assets that `src/document.tsx` imports, such as a font preloaded with `?url`, resolve to their client URL in server-rendered and generated pages. The asset placeholder no longer uses a NUL character, which the HTML parser replaced, and the client build emits an asset that only the document imports.
- Updated dependencies [5aab4af]
- Updated dependencies [0e0d10d]
- Updated dependencies [981d711]
- Updated dependencies [2727fa4]
- Updated dependencies [b8a0cea]
- Updated dependencies [e08592c]
  - @rezejs/compiler@0.3.0
  - @rezejs/router@0.3.0
  - reze-js@0.2.0

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
