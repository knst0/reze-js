---
"@rezejs/compiler": minor
"@rezejs/dom": minor
"@rezejs/vite-plugin": minor
---

Let islands adopt the server DOM when the compiler proves the view is a static element tree with value bindings only. The island claims the existing roots instead of rendering a second copy and swapping it in, so scroll positions, focus, form values and playing media inside it survive the load and the nodes keep their identity. Components, control flow, async components, dynamic tags, `innerHTML`/`textContent` bindings and islands with rendered children keep the replacing path; the browser validates every claimed root and falls back if the server markup does not match.
