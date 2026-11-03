import { guardRoute, loader } from "reze-js/server";
import type { Page } from "virtual:reze-types/app_types";

export const guard = guardRoute.external();

export const load = loader.external<Page>();

export default function Home(props: { data: Page }) {
  return <h1>{props.data.title}</h1>;
}
