---
"@rezejs/vite-plugin": patch
---

Refresh the route table in every dev environment when a route file is added, renamed, or deleted. Previously only the client environment reloaded `virtual:reze-routes`, so server renders kept the old table and failed with `Internal Server Error` until the dev server restarted.
