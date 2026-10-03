---
"@rezejs/signals": minor
"@rezejs/dom": patch
"@rezejs/compiler": minor
"@rezejs/vite-plugin": minor
"reze-js": minor
---

Replace the passive devtools hook with a profile-guided optimization loop. `@rezejs/signals/profile` reports each event with its component, file and node id and collects a serializable per-component session tree (`mounts`/`props`/`reruns`/`writes`) with exact rerun counts. The compiler injects `"file#Component"` attribution into `createComponent` and merges the bind groups of files whose facts show mounts but zero reruns. The Vite plugin reads those facts from `profile.dir` keyed by source hash and files session trees posted to `/__reze/profile` there for the next run. The production branch is unchanged and fully eliminated.
