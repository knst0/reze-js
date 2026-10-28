# reze-js

## 0.2.0

### Minor Changes

- 2727fa4: Prerender up to the first async boundary and stream the rest. `ssg: { ppr: true }` writes a shell with the layouts and fallbacks plus a resume script; the browser fetches the settled parts from the same route with `x-reze-resume: 1`, and the handler streams only the patches below the boundary. `cache(load, { ttl, tags })` keys async data by its JSON arguments on the server, and the server entry exports `revalidateTag(tag)` to drop every entry with that tag.
- e08592c: Add server actions. `server(schema, run)` with a schema from `reze-js/schema` becomes a typed POST endpoint: `.call(input)` posts JSON and resolves to the handler's result, and `.endpoint` works as a `<form method="post">` action without JavaScript, redirecting to `_return` or back with `_action_error`. The compiler rejects a `server` call without an input schema, because every action is a public endpoint; the handler still authorizes the caller. The server entry exposes the endpoint in both the production handler and the dev server.

### Patch Changes

- Updated dependencies [67b1717]
- Updated dependencies [5aab4af]
- Updated dependencies [126debb]
- Updated dependencies [0e0d10d]
- Updated dependencies [2727fa4]
- Updated dependencies [b8a0cea]
- Updated dependencies [e08592c]
  - @rezejs/dom@0.3.0

## 0.1.1

### Patch Changes

- 629483a: Declare the root type entry in the top-level `types` field so npm detects the bundled TypeScript declarations.
- Updated dependencies [37541b7]
- Updated dependencies [8e6e84f]
- Updated dependencies [629483a]
  - @rezejs/dom@0.2.0
  - @rezejs/signals@0.1.1

## 0.1.0

### Minor Changes

- 0162996: Initial release.

### Patch Changes

- Updated dependencies [0162996]
  - @rezejs/dom@0.1.0
  - @rezejs/signals@0.1.0
