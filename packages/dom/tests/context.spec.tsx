import { signal } from "@rezejs/signals";
import { cleanup, fire, mount, settle, tick } from "@rezejs/testing-library";
import { createContext, Errored, useContext } from "reze-js";
import { afterEach, expect, test } from "vite-plus/test";

afterEach(cleanup);

test("a provider supplies its subtree, nesting overrides, the default covers the rest", () => {
  const ThemeContext = createContext<"light" | "dark">("light");
  function Button() {
    return <button class={useContext(ThemeContext)}>x</button>;
  }
  const { el } = mount(() => (
    <main>
      <Button />
      <ThemeContext value="dark">
        <Button />
        <ThemeContext value="light">
          <Button />
        </ThemeContext>
      </ThemeContext>
    </main>
  ));
  expect([...el.querySelectorAll("button")].map((button) => button.className)).toEqual(["light", "dark", "light"]);
});

test("reading a default-less context outside a provider fails with ContextNotFoundError", async () => {
  const Ctx = createContext<string>();
  function Consumer() {
    const value = useContext(Ctx);
    return <>{value}</>;
  }
  async function Page() {
    await Promise.resolve();
    return <Consumer />;
  }
  const { el } = mount(() => (
    <Errored fallback={(error) => <i>{(error as Error).name}</i>}>
      <Page />
    </Errored>
  ));
  await settle();
  expect(el.innerHTML).toBe("<i>ContextNotFoundError</i>");
});
