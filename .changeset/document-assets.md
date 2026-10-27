---
"@rezejs/dom": patch
"@rezejs/vite-plugin": patch
---

Assets that `src/document.tsx` imports, such as a font preloaded with `?url`, resolve to their client URL in server-rendered and generated pages. The asset placeholder no longer uses a NUL character, which the HTML parser replaced, and the client build emits an asset that only the document imports.
