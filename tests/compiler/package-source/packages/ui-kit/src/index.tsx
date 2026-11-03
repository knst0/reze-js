import { signal } from "reze-js";

export { Title } from "./title.tsx";

export function Counter({ start }: { start: number }) {
  let count = signal(start);
  return <button onClick={() => count++}>{count}</button>;
}
