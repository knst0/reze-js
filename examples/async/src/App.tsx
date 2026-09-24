import { Show, Suspense, signal } from "reze-js";

import { UserCard } from "./UserCard";

const IDS = [1, 2, 3];

export function App() {
  const [id, setId] = signal(1);
  return (
    <section class="app">
      <h1>async component</h1>
      <div class="buttons">
        {IDS.map((n) => (
          <button onClick={() => setId(n)} disabled={id() === n}>
            user {n}
          </button>
        ))}
      </div>
      <Show when={id() === 1}>
        <p class="note">Click “user 1” then “user 2” fast: 1 resolves last but never paints.</p>
      </Show>
      <Suspense fallback={<p class="note">loading…</p>}>
        <UserCard id={id()} />
      </Suspense>
    </section>
  );
}
