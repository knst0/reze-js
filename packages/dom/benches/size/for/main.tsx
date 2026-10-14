import { For, render, signal } from "reze-js";

function App() {
  let items = signal(["a", "b"]);
  return (
    <ul onClick={() => (items = [...items, "c"])}>
      <For each={items} fallback={<li>empty</li>}>
        {(item) => <li>{item}</li>}
      </For>
    </ul>
  );
}

render(() => <App />, document.body);
