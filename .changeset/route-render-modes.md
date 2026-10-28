---
"@rezejs/router": minor
"@rezejs/vite-plugin": minor
"@rezejs/dom": minor
---

Choose how a route renders. `export const route = { render: "static" | "server" | "client" }` moves the choice from the build to the route: with `ssg`, `static` leaves become files and `server` leaves are rejected unless `ssr` is built too; `client` renders every layout above the route on the server and mounts a client router for the route subtree, so a static marketing section and a browser-rendered account area live in one application.
