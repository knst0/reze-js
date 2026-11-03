import { For, Show, signal } from "reze-js";
import { Item } from "../item";

export default function Page() {
  let items = signal(["a"]);
  return (
    <main>
      <button onClick={() => (items = [...items, "b"])}>Add</button>
      <ul>
        <For each={items}>{(item) => <Item name={item} />}</For>
      </ul>
      <Show when={items.length > 1}>
        {() => {
          const last = items[items.length - 1];
          return <p>{last}</p>;
        }}
      </Show>
    </main>
  );
}
