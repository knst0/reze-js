import { mutation } from "reze-js/server";

export const createTodo = mutation(async (input: { title: string; done?: boolean }) => input.title);

export default function Home() {
  return (
    <form action={createTodo}>
      <input name="title" />
      <input name="titel" />
      <input name="done" type="checkbox" />
      <button type="submit">Add</button>
    </form>
  );
}
