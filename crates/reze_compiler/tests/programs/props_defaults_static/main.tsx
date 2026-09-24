import { render } from "reze-js";

import { Banner, Hint } from "./components";

render(
  () => (
    <>
      <Banner />
      <Hint />
    </>
  ),
  document.body,
);
