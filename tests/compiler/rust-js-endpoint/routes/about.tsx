import type { RoutePropsFor } from "reze-js";
import { guardRoute, loader } from "reze-js/server";

import { subscribe } from "../actions.ts";

export const guard = guardRoute(async () => {});

export const load = loader(async (): Promise<{ team: string }> => ({ team: "reze" }));

export default function About(props: RoutePropsFor<"/about">) {
  return (
    <form action={subscribe}>
      <p>{props.data.team}</p>
      <input name="email" />
      <button>Subscribe</button>
    </form>
  );
}
