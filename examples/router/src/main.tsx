import { createBrowserHistory, Router } from "@rezejs/router";
import { render } from "reze-js";
import { routes } from "virtual:reze-routes";

import { Shell } from "./Shell";

render(
  () => <Router routes={routes} root={Shell} history={createBrowserHistory(import.meta.env.BASE_URL)} />,
  document.getElementById("app")!,
);
