---
"@rezejs/vite-plugin": minor
"@rezejs/router": minor
---

Render server pages from the file routes alone. `ssg` and `ssr` accept `true` for every default, turn `fileRoutes` on when it is not set, and drop the `entry` option: there is no `src/app.tsx` or server entry module, shared markup lives in the document, and a leftover `entry` fails with a message to remove it. `fileRoutes: false` together with `ssg` or `ssr` is an error. With `fileRoutes` on, `reze()` returns a promise of plugins that Vite's `plugins` array awaits. The server `createContext` factory is gone, along with the `RouterServerContextArgs` and `RouterServerContextFactory` types and the `context` option of server route preparation; a layout's `beforeLoad` supplies those values instead.
