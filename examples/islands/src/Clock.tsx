import { $signal } from "reze-js";

export function Clock() {
  let now = $signal(new Date());
  setInterval(() => (now = new Date()), 1000);
  return <p class="clock">{now.toLocaleTimeString()}</p>;
}
