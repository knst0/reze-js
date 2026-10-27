---
"@rezejs/dom": patch
---

Islands inside the `children` of an island read the context that island provides. The server renders `children` when the island's body reads them, inside its providers, instead of before the body runs, so a part such as `<Menu.Trigger>` no longer fails the page with a missing provider. In the browser, such an island renders under the owner where the enclosing island inserted `children`, instead of in a root of its own.
