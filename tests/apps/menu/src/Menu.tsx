import { For, signal } from "reze-js";

export interface Item {
  href: string;
  label: string;
}

export function Menu(props: { items: Item[] }) {
  let open = signal(false);
  return (
    <nav>
      <button onClick={() => (open = !open)} aria-expanded={open}>
        Menu
      </button>
      <ul hidden={!open}>
        <For each={props.items}>
          {(item) => (
            <li>
              <a href={item.href}>{item.label}</a>
            </li>
          )}
        </For>
      </ul>
    </nav>
  );
}
