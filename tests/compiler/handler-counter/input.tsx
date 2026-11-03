import { For, signal } from "reze-js";

export default function Home() {
  let count = signal(0);
  const items = [
    { id: 1, name: "Tea" },
    { id: 2, name: "Coffee" },
  ];
  return (
    <main>
      <h1>Counter</h1>
      <p>
        Clicked {count} times, label {"x"}
      </p>
      <button onClick={() => count++}>Add</button>
      <a href="/next" onClick={(event) => event.preventDefault()}>
        Next
      </a>
      <ul>
        <For each={items}>{(item) => <li onClick={() => console.log(item.id)}>{item.name}</li>}</For>
      </ul>
    </main>
  );
}
