---
"@rezejs/vite-plugin": minor
"@rezejs/router": patch
---

Leave the `beforeLoad` cascade out of production bundles that never use it. A build defines `__REZE_BEFORE_LOAD__` as `false` when no file in the project mentions `beforeLoad`, and the router drops the cascade from settling and link warming. A dependency that defines `beforeLoad` in such a build fails it with a message to declare that route config in the app's sources. Development and builds without the plugin keep the cascade.
