import { render } from "reze-js";

import { Footer, Header, Tree } from "./components";

render(
  () => (
    <>
      <Header title="Docs" />
      <Tree depth={2} />
      <Footer note="Built" />
    </>
  ),
  document.body,
);
