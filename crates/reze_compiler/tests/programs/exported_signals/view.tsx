import { count, setCount, title, theme } from "./state";
import * as state from "./state";

export function View() {
  return (
    <main class={theme()}>
      <h1>{title()}</h1>
      <p>{state.title()}</p>
      <button onClick={() => setCount(count() + 1)}>{count()}</button>
    </main>
  );
}
