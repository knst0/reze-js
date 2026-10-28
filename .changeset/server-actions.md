---
"reze-js": minor
"@rezejs/compiler": minor
"@rezejs/dom": minor
"@rezejs/vite-plugin": minor
---

Add server actions. `server(schema, run)` with a schema from `reze-js/schema` becomes a typed POST endpoint: `.call(input)` posts JSON and resolves to the handler's result, and `.endpoint` works as a `<form method="post">` action without JavaScript, redirecting to `_return` or back with `_action_error`. The compiler rejects a `server` call without an input schema, because every action is a public endpoint; the handler still authorizes the caller. The server entry exposes the endpoint in both the production handler and the dev server.
