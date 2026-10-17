---
"@rezejs/vite-plugin": patch
---

Pass the request's query string to server routes. `location.query` and `location.search` in `beforeLoad`, `preload` and `redirect` were empty during SSR.
