import { signal } from "@rezejs/signals";
import { cleanup, fire, mount, settle, tick } from "@rezejs/testing-library";
import { onCleanup, Portal, provideContext, Show, useContext } from "reze-js";
import { afterEach, expect, test } from "vite-plus/test";

afterEach(cleanup);

const theme = { id: Symbol("theme"), defaultValue: "light" };

function target(): HTMLElement {
  const el = document.createElement("section");
  document.body.appendChild(el);
  return el;
}

test("Portal keeps its children in the body by default and leaves no trace when disposed", () => {
  const originalNodes = [...document.body.childNodes];
  const { el, dispose } = mount(() => (
    <p>
      <Portal>
        <b>modal</b>
      </Portal>
    </p>
  ));
  expect(el.innerHTML).toBe("<p></p>");
  expect(document.body.querySelector("b")!.textContent).toBe("modal");

  dispose();
  expect(document.body.querySelector("b")).toBeNull();
  expect([...document.body.childNodes]).toEqual([...originalNodes, el]);
});

test("Portal children stay reactive and still see the context of where the Portal is written", () => {
  const [count, setCount] = signal(0);
  const section = target();
  mount(() => (
    <>
      {provideContext(theme, "dark", () => (
        <Portal mount={section}>
          <i>
            {useContext(theme)}:{count()}
          </i>
        </Portal>
      ))}
    </>
  ));
  expect(section.innerHTML).toBe("<i>dark:0</i>");

  setCount(1);
  tick();
  expect(section.innerHTML).toBe("<i>dark:1</i>");
});

test("moving a Portal keeps its components alive, and a dynamic child still updates in the new mount", () => {
  const first = target();
  const second = target();
  const [mountPoint, setMountPoint] = signal<HTMLElement>(first);
  const [label, setLabel] = signal("a");
  const lifecycle: string[] = [];
  function Child() {
    lifecycle.push("create");
    onCleanup(() => lifecycle.push("dispose"));
    return <b>child</b>;
  }
  mount(() => (
    <Portal mount={mountPoint()}>
      {label() ? <Child /> : null}
      {label()}
    </Portal>
  ));
  const node = first.querySelector("b");

  setMountPoint(second);
  tick();
  expect(lifecycle).toEqual(["create"]);
  expect(second.querySelector("b")).toBe(node);

  setLabel("b");
  tick();
  expect(second.innerHTML).toBe("<b>child</b>b");
  expect(first.childNodes.length).toBe(0);
});
