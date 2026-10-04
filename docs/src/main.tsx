import { createBrowserHistory, createRouter } from "@rezejs/router";
import { render } from "reze-js";
import { paths, routes } from "virtual:reze-routes";

import { Shell } from "./Shell";

const Router = createRouter({ routes, paths, history: createBrowserHistory(import.meta.env.BASE_URL) });

render(() => <Router root={Shell} />, document.getElementById("app")!);
