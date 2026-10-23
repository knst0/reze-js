---
"@rezejs/vite-plugin": minor
"@rezejs/router": minor
---

Fill a new empty route file with a starter component during `vite` dev, so a fresh file renders instead of failing for a missing default export. A page types its props by its route pattern; a file beside a same-named directory becomes a layout that renders `children`. `@rezejs/router/fs` exports the generator as `routeTemplate`. Set `fileRoutes.template: false` to turn it off.
