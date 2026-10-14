import { createMemoryHistory, createRouter, defineRoutes, useNavigate } from "@rezejs/router";
import { render } from "reze-js";

function Home() {
  const navigate = useNavigate();
  return <button onClick={() => navigate("/about")}>about</button>;
}

const routes = defineRoutes([
  { path: "/", component: Home },
  { path: "/about", component: () => <p>about</p> },
] as const);

const Router = createRouter({ routes, history: createMemoryHistory() });

render(() => <Router />, document.body);
