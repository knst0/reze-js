import { For, Repeat, Show, signal } from "reze-js";

export default function Page() {
  let open = signal(false);
  let items = signal(["a", "b"]);
  let count = signal(2);
  return (
    <main>
      <button onClick={() => (open = !open)}>Toggle</button>
      <Show when={open} fallback={<p>Closed</p>}>
        <p>Open with {items.length} items</p>
      </Show>
      <ul>
        <For each={items} fallback={<li>Empty</li>}>
          {(item, index) => (
            <li>
              {index + 1}. {item}
              <button onClick={() => (items = items.filter((_, at) => at !== index))}>Drop</button>
            </li>
          )}
        </For>
      </ul>
      <p>
        <For each={items}>{(item) => item}</For>
      </p>
      <p>
        <Repeat count={count}>{() => <span>*</span>}</Repeat>
      </p>
      <button onClick={() => count++}>More</button>
    </main>
  );
}
