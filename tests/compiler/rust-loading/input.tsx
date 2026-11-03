import { Errored, Loading } from "reze-js";
import { loader } from "reze-js/server";
import type { Page } from "virtual:reze-types/app_types";

export const load = loader.external<Page>();

export default function Home(props: { data: Page }) {
  return (
    <main>
      <h1>{props.data.title}</h1>
      <Errored fallback={(error) => <p>{error.message}</p>}>
        <Loading fallback={<p>Counting…</p>}>
          <p>{props.data.count} views</p>
        </Loading>
      </Errored>
    </main>
  );
}
