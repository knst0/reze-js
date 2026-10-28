---
"@rezejs/vite-plugin": minor
"@rezejs/dom": minor
"reze-js": minor
---

Prerender up to the first async boundary and stream the rest. `ssg: { ppr: true }` writes a shell with the layouts and fallbacks plus a resume script; the browser fetches the settled parts from the same route with `x-reze-resume: 1`, and the handler streams only the patches below the boundary. `cache(load, { ttl, tags })` keys async data by its JSON arguments on the server, and the server entry exports `revalidateTag(tag)` to drop every entry with that tag.
