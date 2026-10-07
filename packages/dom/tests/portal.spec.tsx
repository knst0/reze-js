import { signal } from "@rezejs/signals";
import { cleanup, fire, mount, tick } from "@rezejs/testing-library";
import { onCleanup, Portal, provideContext, Show, useContext } from "reze-js";
import { Errored } from "reze-js/internal/async";
import { afterEach, expect, test } from "vitest";

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

test("Portal moves its children to the new mount when a reactive mount changes, keeping their nodes and leaving nothing behind", () => {
  const first = target();
  const second = target();
  const [mountPoint, setMountPoint] = signal<HTMLElement>(first);
  mount(() => (
    <Portal mount={mountPoint()}>
      <u>moved</u>
    </Portal>
  ));
  const node = first.querySelector("u");
  expect(node).not.toBeNull();

  setMountPoint(second);
  tick();
  expect(first.childNodes.length).toBe(0);
  expect(second.innerHTML).toBe("<u>moved</u>");
  expect(second.querySelector("u")).toBe(node);
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

test("a Portal whose mount was emptied by other code still disposes cleanly", () => {
  const section = target();
  const { dispose } = mount(() => (
    <Portal mount={section}>
      <b>x</b>
    </Portal>
  ));
  section.textContent = "";
  dispose();
  expect(section.childNodes.length).toBe(0);
});

test("toggling a Portal many times accumulates nothing in its mount", () => {
  const section = target();
  const [isOpen, setOpen] = signal(false);
  mount(() => (
    <Show when={isOpen()}>
      <Portal mount={section}>
        <b>open</b>
      </Portal>
    </Show>
  ));
  for (let round = 0; round < 3; round++) {
    setOpen(true);
    tick();
    expect(section.innerHTML).toBe("<b>open</b>");
    setOpen(false);
    tick();
    expect(section.childNodes.length).toBe(0);
  }
});

test("a delegated event inside a Portal reaches its handler", () => {
  const clicks: string[] = [];
  const section = target();
  mount(() => (
    <Portal mount={section}>
      <button onClick={() => clicks.push("clicked")}>x</button>
    </Portal>
  ));
  fire(section.querySelector("button")!, "click");
  expect(clicks).toEqual(["clicked"]);
});

test("an error thrown while building Portal children reaches the Errored around it, and what was built is disposed", () => {
  const cleaned: string[] = [];
  const section = target();
  function Boom(): never {
    onCleanup(() => cleaned.push("boom"));
    throw new Error("nope");
  }
  const { el } = mount(() => (
    <Errored fallback={(error) => <em>{(error as Error).message}</em>}>
      <Portal mount={section}>
        <Boom />
      </Portal>
    </Errored>
  ));
  expect(el.innerHTML).toBe("<em>nope</em>");
  expect(section.childNodes.length).toBe(0);
  expect(cleaned).toEqual(["boom"]);
});
