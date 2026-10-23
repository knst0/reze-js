# @rezejs/dom

## 0.2.0

### Minor Changes

- 37541b7: Allow components in `src/document.tsx`, including imported ones. The compiler follows the document's imports into source files and `reze`-condition packages and inlines their components into the static HTML at build time, with props replaced by the literal values passed where they are used, literal constants folded, and `children` replaced by the markup between their tags. A document the compiler cannot read statically no longer fails: the plugin runs it once on the server, at build time and at the first dev render, and `DOCUMENT_DYNAMIC` becomes an informational diagnostic that names the part that made it run.

### Patch Changes

- 8e6e84f: Replay a click that lands on server-rendered island markup while the island's module is still loading. Previously the click hit the static HTML and was lost, so buttons needed a second click right after page load. The click is now held and dispatched on the matching element once the island renders, or on the original element if the load fails.
- 629483a: Declare the root type entry in the top-level `types` field so npm detects the bundled TypeScript declarations.
- Updated dependencies [629483a]
  - @rezejs/signals@0.1.1

## 0.1.0

### Minor Changes

- 0162996: Initial release.

### Patch Changes

- Updated dependencies [0162996]
  - @rezejs/signals@0.1.0
