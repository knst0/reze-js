---
"@rezejs/compiler": minor
"@rezejs/vite-plugin": minor
---

Reject module state written during a request render. The compiler reports `MODULE_STATE_SHARED` as an error on the html target when a component or a route `preload`, `beforeLoad`, `redirect`, `loader`, or `action` writes a module-level `let` or `var`, because that module is loaded once per process and every request shares the value. Values declared with `signal`, `store`, `selector`, `createContext`, `effect`, or `provideContext` keep their per-request copies, and writes inside event handlers stay legal.
