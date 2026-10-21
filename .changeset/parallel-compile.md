---
"@rezejs/compiler": patch
"@rezejs/vite-plugin": patch
---

Compile and analyze modules on the libuv thread pool during Vite builds, so several modules compile in parallel instead of blocking the main thread one at a time. `@rezejs/compiler` exports the promise-returning `compileAsync` and `analyzeAsync`.
