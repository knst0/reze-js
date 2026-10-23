---
"@rezejs/dom": patch
---

Replay a click that lands on server-rendered island markup while the island's module is still loading. Previously the click hit the static HTML and was lost, so buttons needed a second click right after page load. The click is now held and dispatched on the matching element once the island renders, or on the original element if the load fails.
