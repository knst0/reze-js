import { signal } from "@rezejs/signals";
import { $signal, Errored, For, Loading, Match, onCleanup, Show, Switch } from "reze-js";

interface Row {
  id: string;
  label: string;
}
const removedIds: string[] = [];
const [removedText, setRemovedText] = signal("");

function KeyedRow(props: { row: () => Row; index: () => number }) {
  const row = props.row();
  onCleanup(() => {
    removedIds.push(row.id);
    setRemovedText(removedIds.join(","));
  });
  return (
    <li data-row={row.id}>
      <input id={`row-${row.id}`} value={row.label} />
      <span>{props.index()}</span>
    </li>
  );
}

async function Deferred() {
  await Promise.resolve("deferred body");
  return <p id="deferred">deferred settled</p>;
}

async function Fallible(props: { fail: boolean }) {
  const fail = props.fail;
  const text = await Promise.resolve(fail).then((fail) => {
    if (fail) throw new Error("flow failed");
    return "steady";
  });
  return <p id="fallible">{text}</p>;
}

export default function App() {
  let rows = $signal<Row[]>([
    { id: "a", label: "alpha" },
    { id: "b", label: "beta" },
  ]);
  let mode = $signal("one");
  let shown = $signal(true);
  let fail = $signal(false);
  return (
    <main>
      <h1>flows</h1>
      <ul id="rows">
        <For each={rows} keyed={(row) => row.id}>
          {(row, index) => <KeyedRow row={row} index={index} />}
        </For>
      </ul>
      <button id="reorder" type="button" onClick={() => (rows = [...rows].reverse())}>
        reorder
      </button>
      <button id="drop-b" type="button" onClick={() => (rows = rows.filter((row) => row.id !== "b"))}>
        drop b
      </button>
      <button
        id="add-c"
        type="button"
        onClick={() => {
          removedIds.length = 0;
          setRemovedText("");
          rows = [...rows, { id: "c", label: "gamma" }];
        }}
      >
        add c
      </button>
      <p id="removed">{removedText()}</p>
      <Show when={shown} fallback={<p id="show-fallback">hidden</p>}>
        <p id="show-body">visible</p>
      </Show>
      <button id="toggle-show" type="button" onClick={() => (shown = !shown)}>
        toggle
      </button>
      <Switch fallback={<p id="switch-fallback">none</p>}>
        <Match when={mode === "one"}>
          <p id="mode-one">one</p>
        </Match>
        <Match when={mode === "two"}>
          <p id="mode-two">two</p>
        </Match>
      </Switch>
      <button id="mode-two-btn" type="button" onClick={() => (mode = "two")}>
        two
      </button>
      <Loading fallback={<p>loading deferred…</p>}>
        <Deferred />
      </Loading>
      <Errored
        fallback={(error: unknown, reset: () => void) => (
          <p>
            <span id="flow-error">{(error as Error).message}</span>
            <button id="flow-reset" type="button" onClick={reset}>
              reset
            </button>
          </p>
        )}
      >
        <Loading fallback={<p>loading fallible…</p>}>
          <Fallible fail={fail} />
        </Loading>
      </Errored>
      <button id="fail-btn" type="button" onClick={() => (fail = !fail)}>
        fail
      </button>
    </main>
  );
}
