import { render } from "reze-js";

import { Clock } from "./Clock";
import { Counter } from "./Counter";

render(
  () => (
    <main>
      <h1>Islands</h1>
      <p>This text is static: no component code ships for it.</p>
      <Counter island="visible" step={1} islandFallback={<p>loading counter…</p>} />
      <Clock island="idle" />
    </main>
  ),
  document.getElementById("app")!,
);
