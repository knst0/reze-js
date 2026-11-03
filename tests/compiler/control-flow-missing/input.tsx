import { For, Show } from "reze-js";

export default function Page() {
  return (
    <main>
      <Show>
        <p>ok</p>
      </Show>
      <For>{(item) => <p>{item}</p>}</For>
    </main>
  );
}
