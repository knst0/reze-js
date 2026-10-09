import { defineRoute, defineRoutes, useLocation } from "@rezejs/router";
import { signal, untrack } from "reze-js";

let hits = signal(0);

async function Hits(props: { delay: number }) {
  const delay = props.delay;
  untrack(() => {
    hits += 1;
  });
  await new Promise((resolve) => setTimeout(resolve, delay));
  return <p id="hits">{hits}</p>;
}

function Page() {
  const location = useLocation();
  return <Hits delay={Number(location().query.d ?? 0)} />;
}

export const routes = defineRoutes([defineRoute({ path: "/", component: Page })]);
