# @rezejs/dom

## 0.3.0

### Minor Changes

- 5aab4af: Let islands adopt the server DOM when the compiler proves the view is a static element tree with value bindings only. The island claims the existing roots instead of rendering a second copy and swapping it in, so scroll positions, focus, form values and playing media inside it survive the load and the nodes keep their identity. Components, control flow, async components, dynamic tags, `innerHTML`/`textContent` bindings and islands with rendered children keep the replacing path; the browser validates every claimed root and falls back if the server markup does not match.
- 0e0d10d: Keep unchanged route levels across navigation. The server marks every layout and page with a key that carries its id, params, data and pending async work; a swap replaces only from the first level whose key changes, so sidebar scroll, open menus, playing video and island state in a layout survive. An element with `persist="key"` moves from the current page into the matching element of the next one even when its level is replaced. The swap runs inside `document.startViewTransition` when the browser provides it and the user does not prefer reduced motion.
- 2727fa4: Prerender up to the first async boundary and stream the rest. `ssg: { ppr: true }` writes a shell with the layouts and fallbacks plus a resume script; the browser fetches the settled parts from the same route with `x-reze-resume: 1`, and the handler streams only the patches below the boundary. `cache(load, { ttl, tags })` keys async data by its JSON arguments on the server, and the server entry exports `revalidateTag(tag)` to drop every entry with that tag.
- b8a0cea: Choose how a route renders. `export const route = { render: "static" | "server" | "client" }` moves the choice from the build to the route: with `ssg`, `static` leaves become files and `server` leaves are rejected unless `ssr` is built too; `client` renders every layout above the route on the server and mounts a client router for the route subtree, so a static marketing section and a browser-rendered account area live in one application.
- e08592c: Add server actions. `server(schema, run)` with a schema from `reze-js/schema` becomes a typed POST endpoint: `.call(input)` posts JSON and resolves to the handler's result, and `.endpoint` works as a `<form method="post">` action without JavaScript, redirecting to `_return` or back with `_action_error`. The compiler rejects a `server` call without an input schema, because every action is a public endpoint; the handler still authorizes the caller. The server entry exposes the endpoint in both the production handler and the dev server.

### Patch Changes

- 67b1717: Assets that `src/document.tsx` imports, such as a font preloaded with `?url`, resolve to their client URL in server-rendered and generated pages. The asset placeholder no longer uses a NUL character, which the HTML parser replaced, and the client build emits an asset that only the document imports.
- 126debb: Islands inside the `children` of an island read the context that island provides. The server renders `children` when the island's body reads them, inside its providers, instead of before the body runs, so a part such as `<Menu.Trigger>` no longer fails the page with a missing provider. In the browser, such an island renders under the owner where the enclosing island inserted `children`, instead of in a root of its own.

## 0.2.0

### Minor Changes

- 37541b7: Allow components in `src/document.tsx`, including imported ones. The compiler follows the document's imports into source files and `reze`-condition packages and inlines their components into the static HTML at build time, with props replaced by the literal values passed where they are used, literal constants folded, and `children` replaced by the markup between their tags. A document the compiler cannot read statically no longer fails: the plugin runs it once on the server, at build time and at the first dev render, and `DOCUMENT_DYNAMIC` becomes an informational diagnostic that names the part that made it run.

### Patch Changes

- 8e6e84f: Replay a click that lands on server-rendered island markup while the island's module is still loading. Previously the click hit the static HTML and was lost, so buttons needed a second click right after page load. The click is now held and dispatched on the matching element once the island renders, or on the original element if the load fails.
- 629483a: Declare the root type entry in the top-level `types` field so npm detects the bundled TypeScript declarations.
- Updated dependencies [629483a]
  - @rezejs/signals@0.1.1

## 0.1.0

### Minor Changes

- 0162996: Initial release.

### Patch Changes

- Updated dependencies [0162996]
  - @rezejs/signals@0.1.0
