import { signal } from "reze-js";

export function Counter() {
  let count = signal(0);
  return <button onClick={() => count++}>Count {count}</button>;
}
