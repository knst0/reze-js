import { createRouter, defineRoutes } from "@rezejs/router";
import { render } from "reze-js";

const routes = defineRoutes([
  { path: "/", component: () => <a href="/about">about</a> },
  { path: "/about", component: () => <a href="/">home</a> },
] as const);

const Router = createRouter({ routes });

render(() => <Router />, document.body);
