import { defineRoute, defineRoutes } from "@rezejs/router";
import { Head, signal } from "reze-js";

function Home() {
  let n = signal(0);
  return (
    <article>
      <Head>
        <title>Home</title>
      </Head>
      <h1 id="home">home</h1>
      <button id="counter" type="button" onClick={() => (n += 1)}>
        {n}
      </button>
    </article>
  );
}

function About() {
  return (
    <h1 id="about">
      <Head>
        <title>About</title>
      </Head>
      about
    </h1>
  );
}

function News() {
  return (
    <h1 id="news">
      <Head>
        <title>News</title>
      </Head>
      news
    </h1>
  );
}

export const routes = defineRoutes([
  defineRoute({ path: "/", component: Home }),
  defineRoute({ path: "/about", component: About }),
  defineRoute({ path: "/news", component: News }),
  defineRoute<"/context", string, { requestId: string }>({
    path: "/context",
    preload: ({ context }) => context.requestId,
    component: (props) => <h1 id="request-scope">{props.data}</h1>,
  }),
  defineRoute({
    path: "/broken",
    preload: () => {
      throw new Error("broken preload");
    },
    component: () => <h1 id="broken">unreachable</h1>,
  }),
]);
