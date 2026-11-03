import { For } from "reze-js";

export default function Home() {
  const todos = [{ title: "Write" }];
  return (
    <ul>
      <For each={todos}>{(todo) => <li>{todo.title}</li>}</For>
      {todos.map((todo) => (
        <li>{todo.title}</li>
      ))}
    </ul>
  );
}
