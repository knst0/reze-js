import { render, Show, signal } from "reze-js";

function App() {
  let count = signal(0);
  return (
    <button onClick={() => (count += 1)}>
      <Show when={count > 0} fallback={<span>none</span>}>
        <span>some</span>
      </Show>
    </button>
  );
}

render(() => <App />, document.body);
