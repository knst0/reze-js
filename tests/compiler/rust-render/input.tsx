import { buildPaths, For, Show, signal, useLinkState, useLocation, useSearchParams } from "reze-js";
import type { Shop } from "./index.rs";

export const revalidate = 60;

function Price(props: { value: number }) {
  return <span class="price">{props.value}</span>;
}

export default function Home(props: { data: Shop }) {
  let count = signal(0);
  const location = useLocation();
  const search = useSearchParams();
  const home = useLinkState("/");
  return (
    <main>
      <head>
        <title>{props.data.title}</title>
      </head>
      <a href="/">Home</a>
      <a href={props.data.title}>Title</a>
      <p data-home={home.active}>
        {location.pathname}
        {location.search} {search.get("q")}
      </p>
      <Show when={search.has("all")}>
        <p>All</p>
      </Show>
      <h1 class={props.data.open ? "open" : "closed"}>{props.data.title}</h1>
      <ul>
        <For each={props.data.items} fallback={<li>Empty</li>}>
          {(item, index) => (
            <li data-kind={item.kind}>
              {index + 1}. {item.name} <Price value={item.price} />
              <Show when={item.note}>{(note) => <small>{note}</small>}</Show>
              <a href={buildPaths("/items/:name/*rest", { name: item.name, rest: "a/b c" })}>Open</a>
              <a href={buildPaths("/notes/:note?", { note: item.note })}>Note</a>
            </li>
          )}
        </For>
      </ul>
      <p>{props.data.items.length > 0 && "In stock"}</p>
      <button onClick={() => count++}>Added {count}</button>
    </main>
  );
}
