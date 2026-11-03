import { Errored, For, Show } from "reze-js";
import { loader, mutation, useSubmission } from "reze-js/server";

interface NewTodo {
  title: string;
  priority: 1 | 2 | 3;
  tags: string[];
}

export const load = loader(async () => [{ id: 1, title: "Write the spec" }]);

export const createTodo = mutation(async (input: NewTodo) => input.title);

export const toggleTodo = mutation(async (input: { id: number }) => input.id);

export default function Home(props: { data: { id: number; title: string }[] }) {
  const submission = useSubmission(createTodo);
  return (
    <main>
      <Errored fallback={<p>Broken</p>}>
        <ul>
          <For each={props.data}>
            {(todo) => (
              <li>
                <form action={toggleTodo}>
                  <input type="hidden" name="id" value={todo.id} />
                  <button type="submit">{todo.title}</button>
                </form>
              </li>
            )}
          </For>
        </ul>
      </Errored>
      <form action={createTodo}>
        <input name="title" value={submission.input?.title ?? ""} />
        <input name="priority" />
        <input name="tags" />
        <Show when={submission.issues?.title}>{(issue) => <p role="alert">{issue.message}</p>}</Show>
        <button type="submit">Add</button>
      </form>
    </main>
  );
}
