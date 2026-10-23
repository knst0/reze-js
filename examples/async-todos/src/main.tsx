import { Loading, render } from "reze-js";

import { Todos } from "./Todos";

render(
  () => (
    <Loading fallback={<p class="loading">Loading todos…</p>}>
      <Todos />
    </Loading>
  ),
  document.getElementById("app")!,
);
