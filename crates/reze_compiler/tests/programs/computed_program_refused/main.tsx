import { render } from "reze-js";

import { A } from "./a";
import { B } from "./b";
import { Looped } from "./cycle-reader";

export { escapes } from "./state";

render(
  () => (
    <>
      <A />
      <B />
      <Looped />
    </>
  ),
  document.body,
);
