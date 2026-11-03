import { For, Show, signal } from "reze-js";
import { loader } from "reze-js/server";
import type { Shop } from "virtual:reze-types/app_types";

export const load = loader.external<Shop>();

export const revalidate = 60;

function Price(props: { value: number }) {
  return <span class="price">{props.value}</span>;
}

export default function Home(props: { data: Shop }) {
  let count = signal(0);
  return (
    <main>
      <head>
        <title>{props.data.title}</title>
      </head>
      <h1 class={props.data.open ? "open" : "closed"}>{props.data.title}</h1>
      <ul>
        <For each={props.data.items} fallback={<li>Empty</li>}>
          {(item, index) => (
            <li data-kind={item.kind}>
              {index + 1}. {item.name} <Price value={item.price} />
              <Show when={item.note}>{(note) => <small>{note}</small>}</Show>
            </li>
          )}
        </For>
      </ul>
      <p>{props.data.items.length > 0 && "In stock"}</p>
      <button onClick={() => count++}>Added {count}</button>
    </main>
  );
}
