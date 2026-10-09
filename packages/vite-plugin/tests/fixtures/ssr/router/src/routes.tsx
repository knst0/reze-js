import { defineRoute, defineRoutes } from "@rezejs/router";
import { signal } from "reze-js";

function Home() {
  let n = signal(0);
  return (
    <article>
      <h1 id="home">home</h1>
      <button id="counter" type="button" onClick={() => (n += 1)}>
        {n}
      </button>
    </article>
  );
}

function About() {
  return <h1 id="about">about</h1>;
}

function News() {
  return <h1 id="news">news</h1>;
}

export const routes = defineRoutes([
  defineRoute({ path: "/", meta: { title: "Home" }, component: Home }),
  defineRoute({ path: "/about", meta: { title: "About" }, component: About }),
  defineRoute({ path: "/news", meta: { title: "News" }, component: News }),
]);
