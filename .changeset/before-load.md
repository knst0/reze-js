---
"@rezejs/router": minor
---

Add `beforeLoad` to route configs. It runs from the root to the leaf before each level's `preload`, and a returned object is merged into the `context` that the route's own `preload` and `redirect` and every route below it receive. It runs in the browser and on the server alike, including link warming, so a layout such as `src/routes/account.tsx` can share values with its child routes.
