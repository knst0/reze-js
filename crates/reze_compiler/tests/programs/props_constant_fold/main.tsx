import { render } from "reze-js";

import { Counter, Escaped, Spread, Varied } from "./Counter";

const extra = { size: 2 };
export const used = Escaped;

render(
  () => (
    <main>
      <Counter step={1} title="Count" zero={0} />
      <Counter step={1} title="Count" zero={0} />
      <Varied size={1} name="a" />
      <Varied size={2} name="a" />
      <Spread size={1} {...extra} />
      <Escaped size={1} />
    </main>
  ),
  document.getElementById("app")!,
);
