import { For, signal } from "reze-js";

export default function Home() {
  let open = signal(false);
  let name = signal("Ada");
  let picked = signal(0);
  const groups = [
    { id: 1, title: "Fruit", items: [{ id: 10, label: "Pear" }] },
    { id: 2, title: "Tea", items: [{ id: 20, label: "Green" }, { id: 21, label: "Black" }] },
  ];
  return (
    <main>
      <button aria-expanded={open} onClick={() => (open = !open)}>
        Menu
      </button>
      <nav hidden={!open} class={open ? "open" : "closed"}>
        Hello <b>{name}</b>, {name}! <i>static</i> and {picked}
      </nav>
      <input value={name} onInput={(event) => (name = event.currentTarget.value)} />
      <For each={groups}>
        {(group) => (
          <section>
            <h2>{group.title}</h2>
            <For each={group.items}>
              {(item) => (
                <p aria-selected={picked === item.id} onClick={() => (picked = item.id)}>
                  {group.title}: {item.label}
                </p>
              )}
            </For>
          </section>
        )}
      </For>
      <footer>
        Groups: <For each={groups}>{(group) => <span>{group.title}</span>}</For> picked {picked}
      </footer>
    </main>
  );
}
