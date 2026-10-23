# @rezejs/router

## 0.2.0

### Minor Changes

- 91a50e8: Add `beforeLoad` to route configs. It runs from the root to the leaf before each level's `preload`, and a returned object is merged into the `context` that the route's own `preload` and `redirect` and every route below it receive. It runs in the browser and on the server alike, including link warming, so a layout such as `src/routes/account.tsx` can share values with its child routes.
- 1bcd787: Render server pages from the file routes alone. `ssg` and `ssr` accept `true` for every default, turn `fileRoutes` on when it is not set, and drop the `entry` option: there is no `src/app.tsx` or server entry module, shared markup lives in the document, and a leftover `entry` fails with a message to remove it. `fileRoutes: false` together with `ssg` or `ssr` is an error. With `fileRoutes` on, `reze()` returns a promise of plugins that Vite's `plugins` array awaits. The server `createContext` factory is gone, along with the `RouterServerContextArgs` and `RouterServerContextFactory` types and the `context` option of server route preparation; a layout's `beforeLoad` supplies those values instead.
- d7b5a63: Fill a new empty route file with a starter component during `vite` dev, so a fresh file renders instead of failing for a missing default export. A page types its props by its route pattern; a file beside a same-named directory becomes a layout that renders `children`. `@rezejs/router/fs` exports the generator as `routeTemplate`. Set `fileRoutes.template: false` to turn it off.

### Patch Changes

- 8c1e6c6: Leave the `beforeLoad` cascade out of production bundles that never use it. A build defines `__REZE_BEFORE_LOAD__` as `false` when no file in the project mentions `beforeLoad`, and the router drops the cascade from settling and link warming. A dependency that defines `beforeLoad` in such a build fails it with a message to declare that route config in the app's sources. Development and builds without the plugin keep the cascade.
- 316265d: Take a base router `context` per request: `handler(request, { context })` passes it to the routes, each layout's `beforeLoad` extends it, and `preload` and `redirect` read it. Use it for cookies, headers and platform bindings, which `beforeLoad` cannot see. `ssr.devContext(request)` supplies the same value to pages the `vite` dev server renders.
- 629483a: Declare the root type entry in the top-level `types` field so npm detects the bundled TypeScript declarations.
- Updated dependencies [37541b7]
- Updated dependencies [8e6e84f]
- Updated dependencies [629483a]
  - @rezejs/dom@0.2.0
  - reze-js@0.1.1
  - @rezejs/signals@0.1.1

## 0.1.0

### Minor Changes

- 0162996: Initial release.

### Patch Changes

- Updated dependencies [0162996]
  - reze-js@0.1.0
  - @rezejs/dom@0.1.0
  - @rezejs/signals@0.1.0
