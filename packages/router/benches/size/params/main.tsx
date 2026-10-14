import { createMemoryHistory, createRouter, defineRoutes, useParams } from "@rezejs/router";
import { render } from "reze-js";

function Post() {
  const params = useParams<{ id: string }>();
  return <p>post {params.id}</p>;
}

const routes = defineRoutes([
  { path: "/", component: () => <p>home</p> },
  { path: "/posts/:id", component: Post },
] as const);

const Router = createRouter({ routes, history: createMemoryHistory() });

render(() => <Router />, document.body);
