import { signal } from "reze-js";

let clicks = signal(0);

async function fetchText() {
  Reflect.set(globalThis, "__loads", Number(Reflect.get(globalThis, "__loads") ?? 0) + 1);
  return "seeded value";
}

export async function Seeded() {
  const text = await fetchText();
  return (
    <button id="seeded" type="button" onClick={() => (clicks += 1)}>
      {text}:{clicks}
    </button>
  );
}
