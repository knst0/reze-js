---
"@rezejs/vite-plugin": minor
"@rezejs/compiler": minor
---

Render server pages from a static `src/document.tsx` instead of `index.html`. The `ssg`/`ssr` options `template` and `selector` are replaced by `document`; the element that holds `children` is the mount root, and the plugin adds the client script. A leftover `index.html` fails the build with a message to move its head tags into the document. The compiler adds `compileDocument` and the `DOCUMENT_SHAPE`, `DOCUMENT_DYNAMIC` and `DOCUMENT_ROOT` diagnostics. A render that fails before the first byte now answers a `500` page inside the document, and in development shows the Vite error overlay and keeps HMR connected.
