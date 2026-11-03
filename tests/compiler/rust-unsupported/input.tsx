import { loader, mutation } from "reze-js/server";
import type { Page } from "virtual:reze-types/app_types";

export const load = loader.external<Page>();

export const like = mutation(async (input: { id: string }) => input.id);

export default function Home(props: { data: Page }) {
  return (
    <main>
      <h1>{props.data.title}</h1>
      <p>{props.data.missing}</p>
    </main>
  );
}
