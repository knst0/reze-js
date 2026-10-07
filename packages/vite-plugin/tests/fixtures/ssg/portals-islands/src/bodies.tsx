import { onCleanup, signal } from "reze-js";

export function ClockBody() {
  let now = signal(new Date(2026, 9, 4, 12, 0, 0));
  const timer = setInterval(() => (now = new Date(2026, 9, 4, 12, 0, 1)), 500);
  onCleanup(() => clearInterval(timer));
  return (
    <p id="clock" data-ticked="yes">
      {now.toLocaleTimeString()}
    </p>
  );
}

export function EagerBody() {
  return <p id="eager">eager body</p>;
}

export function NeverBody() {
  return <p id="never">never body</p>;
}
