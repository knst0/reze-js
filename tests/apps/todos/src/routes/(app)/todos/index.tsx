import { For, Show } from "reze-js";
import { loader, mutation, useSubmission, type LoaderArgs } from "reze-js/server";

import { todosOf, type Todo } from "../../../db";

export const load = loader(async ({ context, search }: LoaderArgs & { search: { filter?: "open" | "done" } }) =>
  todosOf(context.user!).filter((todo) => search.filter === undefined || todo.done === (search.filter === "done")),
);

export const createTodo = mutation(async (input: { title: string; priority: 1 | 2 | 3 }, { context }) => {
  const list = todosOf(context.user!);
  const todo = { id: list.length + 1, title: input.title, priority: input.priority, done: false };
  list.push(todo);
  return todo;
});

export const toggleTodo = mutation(
  async (input: { id: number }, { context }) => {
    const todo = todosOf(context.user!).find((todo) => todo.id === input.id);
    if (todo) todo.done = !todo.done;
  },
  { revalidate: ["/todos"] },
);

export default function Todos(props: { data: Todo[] }) {
  const submission = useSubmission(createTodo);
  return (
    <main>
      <h1>Todos</h1>
      <ul aria-label="Todos">
        <For each={props.data}>
          {(todo) => (
            <li>
              <a href={`/todos/${todo.id}`}>{todo.title}</a> P{todo.priority} {todo.done ? "done" : "open"}
              <button onClick={() => toggleTodo.call({ id: todo.id })}>Toggle {todo.title}</button>
            </li>
          )}
        </For>
      </ul>
      <form action={createTodo}>
        <label>
          Title <input name="title" value={submission.input?.title ?? ""} />
        </label>
        <label>
          Priority <input name="priority" value={submission.input?.priority ?? "1"} />
        </label>
        <Show when={submission.issues?.priority}>{(issue) => <p role="alert">Priority: {issue.message}</p>}</Show>
        <button type="submit">Add</button>
      </form>
    </main>
  );
}
