import { $signal, For, Loading, Show } from "reze-js";

interface Todo {
  id: number;
  title: string;
  done: boolean;
}

function delay(ms: number): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, ms);
  return promise;
}

async function fetchTodos(): Promise<Todo[]> {
  await delay(600);
  return [
    { id: 1, title: "Learn async components", done: true },
    { id: 2, title: "Try async components", done: false },
    { id: 3, title: "Ship the redesign", done: false },
  ];
}

async function saveTodo(todo: Todo): Promise<void> {
  await delay(800);
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
  let todos = $signal(initial);
  let draft = $signal("");
  let saving = $signal(0);

  const commit = async (change: (list: Todo[]) => Todo[], todo: Todo) => {
    saving += 1;
    try {
      await saveTodo(todo);
      todos = change(todos);
    } finally {
      saving -= 1;
    }
  };

  const toggle = (todo: Todo) => {
    const next = { ...todo, done: !todo.done };
    return commit((list) => list.map((item) => (item.id === todo.id ? next : item)), next);
  };

  const add = () => {
    const title = draft.trim();
    if (title === "") return;
    const fresh = { id: Math.max(...todos.map((todo) => todo.id)) + 1, title, done: false };
    draft = "";
    return commit((list) => [...list, fresh], fresh);
  };

  return (
    <section class="todos">
      <h1>Todos</h1>
      <ul>
        <For each={todos}>
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
      <Show when={saving > 0}>
        <p class="saving">Saving…</p>
      </Show>
      <div class="add">
        <input
          value={draft}
          onInput={(e: InputEvent) => (draft = (e.currentTarget as HTMLInputElement).value)}
          placeholder="Something to do"
        />
        <button onClick={add}>Add</button>
      </div>
    </section>
  );
}
