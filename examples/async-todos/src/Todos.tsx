import { action, computed, For, Show, signal, store } from "reze-js";
import { Loading } from "reze-js/internal/async";

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

async function saveTodo(todo: Todo, isOffline: boolean): Promise<void> {
  await delay(800);
  if (isOffline) throw new Error(`Could not save “${todo.title}”: offline`);
}

function ignore(): void {}

export function Todos() {
  return (
    <Loading fallback={<p class="loading">Loading todos…</p>}>
      <TodoList />
    </Loading>
  );
}

async function TodoList() {
  const initial = await fetchTodos();
  const todos = store(initial);
  let draft = signal("");
  let isOffline = signal(false);

  const toggle = action(async (todo: Todo) => {
    todo.done = !todo.done;
    await saveTodo(todo, isOffline);
  });

  const add = action(async (title: string) => {
    const fresh = { id: Math.max(0, ...todos.map((todo) => todo.id)) + 1, title, done: false };
    todos.push(fresh);
    await saveTodo(fresh, isOffline);
  });

  const submit = () => {
    const title = draft.trim();
    if (title === "") return;
    draft = "";
    add(title).catch(ignore);
  };

  const failure = computed(toggle.error ?? add.error);

  return (
    <section class="todos">
      <h1>Todos</h1>
      <ul>
        <For each={todos}>
          {(todo) => (
            <li class={{ done: todo.done }}>
              <label>
                <input type="checkbox" checked={todo.done} onChange={() => toggle(todo).catch(ignore)} />
                {todo.title}
              </label>
            </li>
          )}
        </For>
      </ul>
      <Show when={toggle.pending + add.pending > 0}>
        <p class="saving">Saving…</p>
      </Show>
      <Show when={failure !== undefined}>
        <p class="error">{String(failure)}</p>
      </Show>
      <label class="offline">
        <input type="checkbox" checked={isOffline} onChange={() => (isOffline = !isOffline)} />
        Offline: saves fail and changes roll back
      </label>
      <div class="add">
        <input
          value={draft}
          onInput={(e: InputEvent) => (draft = (e.currentTarget as HTMLInputElement).value)}
          placeholder="Something to do"
        />
        <button onClick={submit}>Add</button>
      </div>
    </section>
  );
}
