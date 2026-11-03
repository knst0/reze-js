import { loader, notFound, type LoaderArgs } from "reze-js/server";

import { todosOf, type Todo } from "../../../db";

export const load = loader(async ({ params, context, response }: LoaderArgs & { params: { id: number } }) => {
  const todo = todosOf(context.user!).find((todo) => todo.id === params.id);
  if (!todo) notFound();
  response.headers.set("Cache-Control", "private, max-age=60");
  response.cookies.set("last", String(todo.id));
  return todo;
});

export default function TodoPage(props: { data: Todo }) {
  return (
    <main>
      <h1>{props.data.title}</h1>
    </main>
  );
}
