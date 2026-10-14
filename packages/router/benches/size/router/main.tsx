import { createMemoryHistory, createRouter, defineRoutes } from "@rezejs/router";
import { render } from "reze-js";

const routes = defineRoutes([
  { path: "/", component: () => <p>home</p> },
  { path: "/about", component: () => <p>about</p> },
] as const);

const Router = createRouter({ routes, history: createMemoryHistory() });

render(() => <Router />, document.body);
