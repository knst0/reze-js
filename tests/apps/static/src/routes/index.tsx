import { For, Show } from "reze-js";

import { todos } from "../todos";

export default function Home() {
  const done = todos.filter((todo) => todo.done).length;
  return (
    <main>
      <h1>Todos</h1>
      <ul>
        <For each={todos} key="id">
          {(todo) => (
            <li class={todo.done ? "done" : "open"}>
              <a href={`/todos/${todo.id}`}>{todo.title}</a>
            </li>
          )}
        </For>
      </ul>
      <Show when={done} fallback={<p>Nothing done yet</p>}>
        <p>
          {done} of {todos.length} done
        </p>
      </Show>
    </main>
  );
}
