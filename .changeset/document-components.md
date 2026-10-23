---
"@rezejs/compiler": minor
"@rezejs/dom": minor
"@rezejs/vite-plugin": minor
---

Allow components in `src/document.tsx`, including imported ones. The compiler follows the document's imports into source files and `reze`-condition packages and inlines their components into the static HTML at build time, with props replaced by the literal values passed where they are used, literal constants folded, and `children` replaced by the markup between their tags. A document the compiler cannot read statically no longer fails: the plugin runs it once on the server, at build time and at the first dev render, and `DOCUMENT_DYNAMIC` becomes an informational diagnostic that names the part that made it run.
