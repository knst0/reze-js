import { For, Repeat, Show, signal } from "reze-js";

export default function Lists() {
  let open = signal(false);
  let todos = signal(["Milk", "Bread"]);
  let picked = signal("");
  let stars = signal(1);
  return (
    <main>
      <button onClick={() => (open = !open)}>Details</button>
      <Show when={open} fallback={<p>Hidden details</p>}>
        <p>
          Shown with {todos.length} todos <button onClick={() => (stars = stars + 1)}>Star</button>
        </p>
      </Show>
      <button onClick={() => (todos = [...todos, `Item ${todos.length + 1}`])}>Push</button>
      <button onClick={() => (todos = [...todos].reverse())}>Reverse</button>
      <ul aria-label="Todos">
        <For each={todos} fallback={<li>Nothing to do</li>}>
          {(todo, index) => (
            <li aria-selected={picked === todo}>
              {index + 1}. {todo}
              <button onClick={() => (picked = todo)}>Pick {todo}</button>
              <button onClick={() => (todos = todos.filter((_, at) => at !== index))}>Drop {todo}</button>
              <Show when={picked === todo}>
                <em>picked</em>
              </Show>
            </li>
          )}
        </For>
      </ul>
      <p aria-label="Tags">
        <For each={todos}>{(todo) => todo}</For>
      </p>
      <p aria-label="Stars">
        <Repeat count={stars}>{() => <span>*</span>}</Repeat>
      </p>
    </main>
  );
}
