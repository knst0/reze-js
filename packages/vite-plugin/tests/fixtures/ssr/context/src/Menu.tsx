import { createContext, signal, useContext, type JSX } from "reze-js";

const MenuContext = createContext<{ isOpen: () => boolean; toggle: () => void }>();

export function Root(props: { children: JSX.Element }) {
  let open = signal(false);
  return (
    <div id="root" onKeyDown={() => (open = false)}>
      <MenuContext value={{ isOpen: () => open, toggle: () => (open = !open) }}>{props.children}</MenuContext>
    </div>
  );
}

export function Trigger() {
  const menu = useContext(MenuContext);
  return (
    <button id="trigger" type="button" onClick={menu.toggle}>
      {menu.isOpen() ? "open" : "closed"}
    </button>
  );
}

export function Content() {
  const menu = useContext(MenuContext);
  return (
    <p id="content" onKeyDown={menu.toggle}>
      {menu.isOpen() ? "shown" : "hidden"}
    </p>
  );
}
