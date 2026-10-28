---
"@rezejs/dom": minor
"@rezejs/router": minor
"@rezejs/vite-plugin": minor
---

Keep unchanged route levels across navigation. The server marks every layout and page with a key that carries its id, params, data and pending async work; a swap replaces only from the first level whose key changes, so sidebar scroll, open menus, playing video and island state in a layout survive. An element with `persist="key"` moves from the current page into the matching element of the next one even when its level is replaced. The swap runs inside `document.startViewTransition` when the browser provides it and the user does not prefer reduced motion.
