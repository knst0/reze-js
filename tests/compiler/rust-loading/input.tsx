import { Errored, Loading } from "reze-js";
import type { Page } from "./index.rs";

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
