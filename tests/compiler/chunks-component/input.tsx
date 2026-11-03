import { signal } from "reze-js";

import { Counter } from "../components/counter";

export default function Home() {
  let open = signal(false);
  return (
    <main>
      <button aria-expanded={open} onClick={() => (open = !open)}>
        Menu
      </button>
      <Counter />
    </main>
  );
}
