import { setTodo, settings, todo, whole, setWhole } from "./todo";

export function TodoView() {
  return (
    <li class={settings.theme}>
      <input
        type="checkbox"
        checked={todo.meta.done}
        onInput={() =>
          setTodo((d) => {
            d.meta.done = !d.meta.done;
          })
        }
      />
      {todo.title} ({todo.meta["due-date"]})
      <button onClick={() => setWhole((d) => d.count++)}>{JSON.stringify(whole)}</button>
    </li>
  );
}
