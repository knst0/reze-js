import { signal as runtimeSignal } from "@rezejs/signals";
import { action, computed, render, signal } from "reze-js";

import { ModuleClass, moduleBranchValue, moduleCatchValue, moduleGetter, moduleTrace } from "./module-resources.js";

const shared = { items: ["x"] };
const log: string[] = [];
let liveStarted = false;

if (typeof document !== "undefined" && document.getElementById("live") !== null && !liveStarted) {
  liveStarted = true;
  const [ticks, setTicks] = runtimeSignal(0);
  setInterval(() => setTicks((n) => n + 1), 50);
  render(() => <p id="live-count">{ticks()}</p>, document.getElementById("live")!);
}

await (typeof document === "undefined" ? Promise.resolve() : fetch("/module-ready.json"));

async function First({ note }: { note: () => void }) {
  const before = shared;
  await new Promise((done) => setTimeout(done, 50));
  log.push("A");
  before.items.push("y");
  note();
  return <span id="first">{before === shared ? "same" : "diff"}</span>;
}
First.pending = <p>loading order…</p>;

async function Second({ note }: { note: () => void }) {
  await Promise.resolve();
  log.push("B");
  note();
  return <span id="second">{shared.items.join(",")}</span>;
}
Second.pending = <p>loading order…</p>;

async function Slow() {
  await new Promise((done) => setTimeout(done, 600));
  return <p id="slow">slow settled</p>;
}
Slow.pending = <p>loading slow…</p>;

async function Flaky(props: { round: number }) {
  const round = props.round;
  const text = await Promise.resolve(round).then(async (round) => {
    if (typeof document !== "undefined") await fetch(`/round-probe.json?round=${round}`);
    if (round !== 0) throw new Error("flaky failed");
    return "ok";
  });
  return (
    <b id="flaky" ref={(node) => node.setAttribute("data-hydrated", "")}>
      {text}
    </b>
  );
}
Flaky.pending = <p>loading flaky…</p>;
Flaky.failure = (error: unknown, retry: () => void) => (
  <p>
    <span id="flaky-error">{(error as Error).message}</span>
    <button id="retry" type="button" onClick={retry}>
      retry
    </button>
  </p>
);

async function nestedInput(value: number): Promise<number> {
  if (typeof document !== "undefined") {
    const response = await fetch(`/round-probe.json?nested=${value}`);
    if (!response.ok) throw new Error("nested input failed");
  }
  return value;
}

function failOperand(): never {
  if (typeof document !== "undefined") void fetch("/round-probe.json?nested=sync");
  throw new Error("sync");
}

function NestedActions() {
  let value = signal(0);
  let rejection = signal("");
  let syncRuns = signal(0);
  let syncTrace = signal("");
  const run = action(async () => {
    value = await nestedInput((typeof document === "undefined" ? await nestedInput(2) : await nestedInput(9)) + (await nestedInput(3)));
  });
  const recover = action(async () => {
    try {
      await nestedInput(await Promise.reject(new Error("nested rejection")));
    } catch (error) {
      rejection = (error as Error).message;
    } finally {
      rejection += ":finally";
    }
  });
  const sync = action(async () => {
    syncRuns += 1;
    syncTrace = "";
    queueMicrotask(() => (syncTrace += ":microtask"));
    try {
      await failOperand();
    } catch (error) {
      syncTrace += (error as Error).message;
    } finally {
      syncTrace += ":finally";
    }
  });
  run();
  recover();
  sync();
  return (
    <section>
      <p id="nested-value" ref={(node) => node.setAttribute("data-hydrated", "")}>
        {value}
      </p>
      <p id="nested-rejection">{rejection}</p>
      <button id="nested-run" type="button" onClick={run}>
        run nested action
      </button>
      <p id="nested-sync-result">
        {syncRuns}:{syncTrace}
      </p>
      <button id="nested-sync" type="button" onClick={sync}>
        run synchronous operand
      </button>
    </section>
  );
}

export default function App() {
  let count = signal(2);
  let step = signal(7);
  let derived = computed(count * 2);
  let other = computed(step + 1);
  let round = signal(0);
  let done = signal(0);
  const note = (): void => {
    done = done + 1;
  };
  return (
    <main>
      <h1>reactive</h1>
      <p id="module-branch" ref={(node) => node.setAttribute("data-hydrated", "")}>
        {moduleBranchValue()}
      </p>
      <p id="module-getter" ref={(node) => node.setAttribute("data-hydrated", "")}>
        {moduleGetter()}
      </p>
      <p id="module-class" ref={(node) => node.setAttribute("data-hydrated", "")}>
        {ModuleClass.value()}
      </p>
      <p id="module-catch" ref={(node) => node.setAttribute("data-hydrated", "")}>
        {moduleCatchValue()}
      </p>
      <p id="module-runs">{moduleTrace()}</p>
      <p id="counts">
        {count}:{step}:{derived}:{other}
      </p>
      <button id="step" type="button" onClick={() => ((count += 1), (step += 1))}>
        step
      </button>
      <First note={note} />
      <Second note={note} />
      <p id="order">{done === 2 ? log.join(",") : "pending"}</p>
      <NestedActions />
      <Slow />
      <Flaky round={round} />
      <button id="break" type="button" onClick={() => (round = round === 0 ? 1 : 0)}>
        break
      </button>
    </main>
  );
}
