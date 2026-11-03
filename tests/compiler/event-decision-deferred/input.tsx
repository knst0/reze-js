import { signal } from "reze-js";

export default function Home() {
  let dirty = signal(false);
  return (
    <main>
      <input onInput={() => (dirty = true)} />
      <a
        href="/next"
        onClick={(event) => {
          if (dirty) event.preventDefault();
        }}
      >
        Next
      </a>
      <a
        href="/later"
        eager
        onClick={(event) => {
          if (dirty) event.preventDefault();
        }}
      >
        Later
      </a>
    </main>
  );
}
