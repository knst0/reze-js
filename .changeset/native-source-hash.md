---
"@rezejs/compiler": patch
"@rezejs/vite-plugin": patch
---

Speed up builds by hashing module sources in the native compiler instead of with BigInt arithmetic in JavaScript, and by reading and hashing each imported file once per change instead of on every importer lookup. `@rezejs/compiler` exports the hash as `profileHash`.
