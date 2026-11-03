import { signal } from "reze-js";
import { loader, mutation, MutationInputError, notFound, type LoaderArgs } from "reze-js/server";

import { todosOf, type Todo } from "../../../db";

export const load = loader(async ({ params, context, response }: LoaderArgs & { params: { id: number } }): Promise<Todo> => {
  const todo = todosOf(context.user!).find((todo) => todo.id === params.id);
  if (!todo) notFound();
  response.headers.set("Cache-Control", "private, max-age=60");
  response.cookies.set("last", String(todo.id));
  return todo;
});

export const renameTodo = mutation(async (input: { id: number; title: string; priority: 1 | 2 | 3 }, { context }) => {
  const todo = todosOf(context.user!).find((todo) => todo.id === input.id);
  if (todo) Object.assign(todo, { title: input.title, priority: input.priority });
  return { length: input.title.length };
});

export default function TodoPage(props: { data: Todo }) {
  let title = signal(props.data.title);
  let priority = signal("1");
  let message = signal("");
  return (
    <main>
      <h1>{props.data.title}</h1>
      <p>Priority {props.data.priority}</p>
      <label>
        New title <input value={title} onInput={(event) => (title = event.currentTarget.value)} />
      </label>
      <label>
        New priority <input value={priority} onInput={(event) => (priority = event.currentTarget.value)} />
      </label>
      <button
        onClick={async () => {
          try {
            const result = await renameTodo.call({ id: props.data.id, title, priority: Number(priority) as 1 | 2 | 3 });
            message = `Renamed with ${result.length} characters`;
          } catch (error) {
            message = error instanceof MutationInputError ? `Invalid ${error.issues[0]!.path.join(".")}` : "Failed";
          }
        }}
      >
        Rename
      </button>
      <p role="status">{message}</p>
    </main>
  );
}
