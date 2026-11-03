import { effect, signal, untrack } from "reze-js";

export default function Home() {
  let page = signal("home");
  let user = signal("ada");
  effect(() => {
    console.log(page, untrack(() => user));
  });
  const initial = untrack(() => user);
  return (
    <main>
      <p>{initial}</p>
      <button onClick={() => console.log(untrack(() => page))}>Log</button>
      <button onClick={() => (user = "grace")}>Switch</button>
    </main>
  );
}
