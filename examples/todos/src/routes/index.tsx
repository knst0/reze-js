import { For, Show } from "reze-js";
import { loader, mutation, useSubmission } from "reze-js/server";

import { db, type Todo } from "../db";

export const prerender = false;

export const load = loader(async () => db.all());

export const addTodo = mutation(async (input: { title: string }) => {
  const title = input.title.trim();
  if (title !== "") await db.add(title);
});

export const toggleTodo = mutation(async (input: { id: number }) => db.toggle(input.id));

export const removeTodo = mutation(async (input: { id: number }) => db.remove(input.id));

export default function Todos(props: { data: Todo[] }) {
  const submission = useSubmission(addTodo);
  return (
    <main>
      <h1>Todos</h1>
      <ul aria-label="Todos">
        <For each={props.data} fallback={<li>Nothing to do</li>}>
          {(todo) => (
            <li class={todo.done ? "done" : "open"}>
              <a href={`/todos/${todo.id}`}>{todo.title}</a>
              <form action={toggleTodo}>
                <input type="hidden" name="id" value={todo.id} />
                <button type="submit">{todo.done ? "Reopen" : "Done"}</button>
              </form>
              <form action={removeTodo}>
                <input type="hidden" name="id" value={todo.id} />
                <button type="submit">Remove</button>
              </form>
            </li>
          )}
        </For>
      </ul>
      <form action={addTodo}>
        <label>
          Title <input name="title" required value={submission.input?.title ?? ""} />
        </label>
        <Show when={submission.issues?.title}>{(issue) => <p role="alert">{issue.message}</p>}</Show>
        <button type="submit">Add</button>
      </form>
    </main>
  );
}
