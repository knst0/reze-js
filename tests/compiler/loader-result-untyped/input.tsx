import { signal, type RoutePropsFor } from "reze-js";
import { loader } from "reze-js/server";

export const load = loader(async () => ({ greeting: "Hello" }));

export default function Home(props: RoutePropsFor<"/">) {
  let shown = signal("");
  return (
    <main>
      <button onClick={() => (shown = props.data.greeting)}>Greet</button>
      <output>{shown}</output>
    </main>
  );
}
