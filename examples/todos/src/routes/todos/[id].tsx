import { loader, notFound, type LoaderArgs } from "reze-js/server";

import { db, type Todo } from "../../db";

export const load = loader(async ({ params }: LoaderArgs & { params: { id: number } }) => {
  const todo = await db.get(params.id);
  if (!todo) notFound();
  return todo;
});

export default function TodoPage(props: { data: Todo }) {
  return (
    <main>
      <head>
        <title>{props.data.title}</title>
      </head>
      <h1>{props.data.title}</h1>
      <p>{props.data.done ? "Done" : "Open"}</p>
    </main>
  );
}
