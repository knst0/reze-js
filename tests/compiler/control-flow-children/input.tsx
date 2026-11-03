import { For, Match, Switch } from "reze-js";

export default function Page() {
  const tags = ["a"];
  return (
    <main>
      <ul>
        <For each={tags}>
          <li />
        </For>
      </ul>
      <Switch>
        <p>loose</p>
        <Match when={tags.length}>
          <p>some</p>
        </Match>
      </Switch>
    </main>
  );
}
