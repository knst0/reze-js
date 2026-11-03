import { For, Match, Repeat, Show, Switch } from "reze-js";

export default function Home() {
  const todos = [
    { id: 1, title: "Write", done: true },
    { id: 2, title: "Ship", done: false },
  ];
  const user: { name: string } | null = { name: "Ann" };
  const empty: string[] = [];
  const status = "ready";
  return (
    <main>
      <ul>
        <For each={todos} key="id">
          {(todo) => <li class={todo.done ? "done" : "open"}>{todo.title}</li>}
        </For>
      </ul>
      <For each={empty} fallback={<p>Nothing</p>}>
        {(item) => <p>{item}</p>}
      </For>
      <Show when={user} fallback={<p>Signed out</p>}>
        {(user) => <p>Hi {user.name}</p>}
      </Show>
      <Show when={empty.length}>
        <p>never</p>
      </Show>
      <Switch fallback={<p>unknown</p>}>
        <Match when={status === "loading"}>
          <p>loading</p>
        </Match>
        <Match when={status === "ready"}>
          <p>ready</p>
        </Match>
      </Switch>
      <Repeat count={3}>{(index) => <i>{index}</i>}</Repeat>
    </main>
  );
}
