import { render } from "reze-js";

import App from "./routes/index";

const root = document.getElementById("app");
if (root === null) throw new Error("[reze-test] CSR mount element is missing");
render(() => <App />, root);
