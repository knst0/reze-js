import { render } from "reze-js";

import { name } from "./entry-export";
import { kept } from "./globbed";
import * as opened from "./opened";

export { shown } from "./entry-export";
export const modules = import.meta.glob("./globbed.ts");

console.log(opened);
render(
  () => (
    <p>
      {name()} {opened.value()} {kept()}
    </p>
  ),
  document.body,
);
