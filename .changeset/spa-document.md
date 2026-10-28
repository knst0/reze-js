---
"@rezejs/vite-plugin": minor
---

Render client-only pages from `src/document.tsx` instead of `index.html`. Without `ssg` or `ssr`, the plugin compiles the document into the HTML entry, serves it from the dev server for every path that accepts `text/html`, and reloads the page when the document or a component it inlines changes. The document must be static, because the plugin compiles it into the entry instead of running it on a server. A leftover `index.html` next to the document fails the build with a message to move its tags into the document. A project with an explicit bundler input or without `src/document.tsx` keeps Vite's `index.html`.
