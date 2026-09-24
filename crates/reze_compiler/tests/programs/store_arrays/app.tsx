import { setState, state } from "./store";

export function App() {
  return (
    <ul
      onClick={() =>
        setState((d) => {
          d.todos.push({ done: true });
          d.todos[0].done = true;
        })
      }
    >
      <For each={state.todos}>{(item) => <li>{item().done}</li>}</For>
      {state.todos.length}
      {state.todos[0].done}
    </ul>
  );
}
