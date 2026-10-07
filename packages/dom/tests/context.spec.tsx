import { signal } from "@rezejs/signals";
import { cleanup, fire, mount, tick } from "@rezejs/testing-library";
import { createContext, useContext } from "reze-js";
import { Errored } from "reze-js/internal/async";
import { afterEach, expect, test } from "vitest";

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

test("a default-less context scopes a reactive payload to its subtree", () => {
  type TodosCtx = readonly [() => string[], { add: (todo: string) => void }];
  const TodosContext = createContext<TodosCtx>();
  function TodoList() {
    const [todos, { add }] = useContext(TodosContext);
    return (
      <li>
        {todos().join(",")}
        <button onClick={() => add("b")}>add</button>
      </li>
    );
  }
  function App() {
    const [todos, setTodos] = signal(["a"]);
    const value: TodosCtx = [todos, { add: (todo) => setTodos([...todos(), todo]) }];
    return (
      <TodosContext value={value}>
        <TodoList />
      </TodosContext>
    );
  }
  const { el } = mount(() => <App />);
  expect(el.innerHTML).toBe("<li>a<button>add</button></li>");
  fire(el.querySelector("button")!, "click");
  tick();
  expect(el.innerHTML).toBe("<li>a,b<button>add</button></li>");
});
test("reading a default-less context outside a provider throws", () => {
  const Ctx = createContext<string>();
  function Consumer() {
    const value = useContext(Ctx);
    return <>{value}</>;
  }
  const { el } = mount(() => (
    <Errored fallback={(error) => <i>{(error as Error).name}</i>}>
      <Consumer />
    </Errored>
  ));
  expect(el.innerHTML).toBe("<i>ContextNotFoundError</i>");
});
