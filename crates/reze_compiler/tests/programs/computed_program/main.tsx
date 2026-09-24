import { render } from "reze-js";

import { Badge } from "./badge";
import { View } from "./view";

render(
  () => (
    <>
      <View />
      <Badge />
    </>
  ),
  document.body,
);
