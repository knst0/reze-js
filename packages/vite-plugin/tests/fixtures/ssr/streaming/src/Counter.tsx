import { signal } from "reze-js";

export function Counter() {
  let n = signal(0);
  return (
    <button id="counter" type="button" onClick={() => (n += 1)}>
      {n}
    </button>
  );
}
