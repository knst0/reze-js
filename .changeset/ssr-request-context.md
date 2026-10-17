---
"@rezejs/vite-plugin": minor
"@rezejs/router": patch
---

Take a base router `context` per request: `handler(request, { context })` passes it to the routes, each layout's `beforeLoad` extends it, and `preload` and `redirect` read it. Use it for cookies, headers and platform bindings, which `beforeLoad` cannot see. `ssr.devContext(request)` supplies the same value to pages the `vite` dev server renders.
