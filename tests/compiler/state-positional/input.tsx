import { For, signal } from "reze-js";
import { loader } from "reze-js/server";

type Status = "open" | "done";

type Shape = { kind: "circle"; radius: number } | { kind: "square"; side: number; at?: Date };

interface Todo {
  title: string;
  status: Status;
  due?: Date;
  note?: string | null;
  secret: string;
}

interface Board {
  todos: Todo[];
  shape: Shape;
  owner: { name: string; since: Date; badge: Uint8Array };
}

export const load = loader(
  async (): Promise<Board> => ({
    todos: [
      { title: "Milk", status: "open", note: null, secret: "a" },
      { title: "Bread", status: "done", due: new Date(0), secret: "b" },
    ],
    shape: { kind: "square", side: 2 },
    owner: { name: "Ann", since: new Date(0), badge: new Uint8Array([1, 2]) },
  }),
);

export default function Page(props: { data: Board }) {
  let todos = signal(props.data.todos);
  return (
    <main>
      <ul>
        <For each={todos}>{(todo) => <li>{todo.title}</li>}</For>
      </ul>
      <button onClick={() => (todos = todos.filter((todo) => todo.status === "open"))}>Open</button>
      <button onClick={() => console.log(props.data.shape, props.data.owner)}>Log</button>
    </main>
  );
}
