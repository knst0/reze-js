import { onCleanup, signal } from "reze-js";

export function Clock() {
  let now = signal(new Date());
  const timer = setInterval(() => (now = new Date()), 1000);
  onCleanup(() => clearInterval(timer));
  return <p class="clock">{now.toLocaleTimeString()}</p>;
}
