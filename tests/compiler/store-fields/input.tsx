import { For, store } from "reze-js";

export default function Counts() {
  const counts = store({ apples: 0, pears: 0 });
  const todos = store([{ title: "Milk", done: false }]);
  return (
    <main>
      <p aria-label="Apples">{counts.apples}</p>
      <p aria-label="Pears">{counts.pears}</p>
      <button onClick={() => counts.apples++}>Apple</button>
      <button onClick={() => counts.pears++}>Pear</button>
      <ul aria-label="Todos">
        <For each={todos}>
          {(todo) => (
            <li>
              {todo.title} {todo.done ? "done" : "open"}
            </li>
          )}
        </For>
      </ul>
      <p aria-label="Count">{todos.length} todos</p>
      <button onClick={() => todos.push({ title: `Todo ${todos.length + 1}`, done: false })}>Push</button>
      <button onClick={() => (todos[0]!.done = true)}>Finish</button>
      <button onClick={() => todos.splice(0, todos.length)}>Clear</button>
    </main>
  );
}
