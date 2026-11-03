import { For, signal } from "reze-js";

import { Menu } from "../Menu";

const fruits = [
  { id: 1, name: "Apple" },
  { id: 2, name: "Pear" },
  { id: 3, name: "Plum" },
];

export default function Home() {
  let count = signal(0);
  let selected = signal(0);
  let message = signal("");
  return (
    <main>
      <Menu
        items={[
          { href: "/docs", label: "Docs" },
          { href: "/blog", label: "Blog" },
        ]}
      />
      <p>Clicked {count} times</p>
      <button onClick={() => count++}>Add</button>
      <ul aria-label="Fruits">
        <For each={fruits}>
          {(fruit) => (
            <li aria-selected={selected === fruit.id} onClick={() => (selected = fruit.id)}>
              {fruit.name}
            </li>
          )}
        </For>
      </ul>
      <a
        href="/elsewhere"
        onClick={(event) => {
          event.preventDefault();
          message = "Stayed";
        }}
      >
        Stay here
      </a>
      <output>{message}</output>
    </main>
  );
}
