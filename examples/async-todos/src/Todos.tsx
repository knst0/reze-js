import { For, Loading, optimistic, signal } from "reze-js";

interface Todo {
  id: number;
  title: string;
  done: boolean;
}

async function fetchTodos(): Promise<Todo[]> {
  await new Promise((resolve) => setTimeout(resolve, 600));
  return [
    { id: 1, title: "Learn async components", done: true },
    { id: 2, title: "Try optimistic updates", done: false },
    { id: 3, title: "Ship the redesign", done: false },
  ];
}

async function saveTodo(todo: Todo): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 800));
}

export function Todos() {
  return (
    <Loading fallback={<p class="loading">Loading todos…</p>}>
      <TodoList />
    </Loading>
  );
}

async function TodoList() {
  const initial = await fetchTodos();
  const [saved, setSaved] = signal(initial);
  const [todos, layer] = optimistic(saved);
  const [draft, setDraft] = signal("");

  const commit = (change: (list: Todo[]) => Todo[], save: Promise<void>) => {
    const drop = layer(change);
    save.then(() => {
      setSaved(change);
      drop();
    }, drop);
  };

  const toggle = (todo: Todo) => {
    commit((list) => list.map((item) => (item.id === todo.id ? { ...item, done: !item.done } : item)), saveTodo({ ...todo, done: !todo.done }));
  };

  const add = () => {
    const title = draft().trim();
    if (title === "") return;
    const fresh = { id: Math.max(...todos().map((todo) => todo.id)) + 1, title, done: false };
    setDraft("");
    commit((list) => [...list, fresh], saveTodo(fresh));
  };

  return (
    <section class="todos">
      <h1>Todos</h1>
      <ul>
        <For each={todos()}>
          {(todo) => (
            <li class={{ done: todo().done }}>
              <label>
                <input type="checkbox" checked={todo().done} onChange={() => toggle(todo())} />
                {todo().title}
              </label>
            </li>
          )}
        </For>
      </ul>
      <div class="add">
        <input value={draft()} onInput={(e: InputEvent) => setDraft((e.currentTarget as HTMLInputElement).value)} placeholder="Something to do" />
        <button onClick={add}>Add</button>
      </div>
    </section>
  );
}
