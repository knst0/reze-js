import { mutation } from "reze-js/server";

interface Todo {
  id: number;
  title: string;
}

export const rename = mutation(async (input: Pick<Todo, "title">) => input.title);

export const schedule = mutation(async (input: { when: Temporal.Instant }) => input.when);

export default function Home() {
  return <main>Home</main>;
}
