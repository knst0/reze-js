import { For, signal } from "reze-js";
import { loader, mutation } from "reze-js/server";

export const load = loader(async () => [{ id: 1, title: "Write the spec" }]);

export const toggleTodo = mutation(async (input: { id: number }) => input.id);

export const clearDone = mutation(async (input: {}) => input);

export const rename = mutation(async (input: { id: number; title: string; urgent: boolean }) => input.id);

export default function Home(props: { data: { id: number; title: string }[] }) {
  let draft = signal("");
  return (
    <main>
      <ul>
        <For each={props.data}>
          {(todo) => (
            <li>
              <button class="toggle" onClick={() => toggleTodo.call({ id: todo.id })}>
                {todo.title}
              </button>
              <button onClick={() => rename({ id: todo.id, title: draft, urgent: false })}>Rename</button>
            </li>
          )}
        </For>
      </ul>
      <button onClick={async () => {
        await clearDone({});
      }}>Clear</button>
      <button type="button" onClick={() => toggleTodo({ id: 1 })}>Kept</button>
      <input onInput={(event) => (draft = event.currentTarget.value)} />
    </main>
  );
}
