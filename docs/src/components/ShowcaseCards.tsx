import { signal, type Getter, type Setter } from "@rezejs/signals";
import { $action, $signal, createContext, For, Show, store, useContext } from "reze-js";

const ShowcaseContext = createContext<{ online: Getter<boolean>; setOnline: Setter<boolean> }>();

export function ShowcaseCards() {
  let [online, setOnline] = signal(true);
  let canPrev = $signal(false);
  let canNext = $signal(false);
  let track: HTMLDivElement | undefined;

  const sync = () => {
    if (!track) return;
    canPrev = track.scrollLeft > 0;
    canNext = track.scrollLeft < track.scrollWidth - track.clientWidth - 1;
  };
  const attach = (el: HTMLDivElement) => {
    track = el;
    el.addEventListener("scroll", sync, { passive: true });
    requestAnimationFrame(sync);
  };
  const go = (direction: 1 | -1) => {
    if (!track) return;
    const card = track.firstElementChild as HTMLElement | null;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    track.scrollBy({ left: direction * ((card?.offsetWidth ?? 0) + (Number.parseFloat(getComputedStyle(track).columnGap) || 0)), behavior: reduced ? "auto" : "smooth" });
  };

  return (
    <section class="showcase-bleed">
      <div class="showcase-inset mb-3 flex justify-end gap-2">
        <button type="button" class="btn px-3" aria-label="Previous card" disabled={!canPrev} onClick={() => go(-1)}>
          ←
        </button>
        <button type="button" class="btn px-3" aria-label="Next card" disabled={!canNext} onClick={() => go(1)}>
          →
        </button>
      </div>
      <div ref={(el: HTMLDivElement) => attach(el)} class="showcase-track flex gap-4 overflow-x-auto overscroll-x-contain snap-x snap-mandatory pb-1">
        <ShowcaseContext value={{ online, setOnline }}>
          <figure class="card showcase-first w-88 h-96 shrink-0 grid place-items-center snap-start">
            <Counter />
          </figure>
          <figure class="card w-88 h-96 shrink-0 grid place-items-center snap-start">
            <Todos />
          </figure>
          <figure class="card showcase-last w-88 h-96 shrink-0 snap-start">
            <ThousandRows />
          </figure>
        </ShowcaseContext>
      </div>
    </section>
  );
}

function Counter() {
  const context = useContext(ShowcaseContext);
  let count = $signal(0);
  return (
    <div class="flex items-center">
      <button type="button" class="btn" onClick={() => (count -= 1)} disabled={!context.online()}>
        -1
      </button>
      <output class="text-2xl tabular-nums w-16 text-center">{count}</output>
      <button type="button" class="btn" onClick={() => (count += 1)} disabled={!context.online()}>
        +1
      </button>
    </div>
  );
}

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

type Todo = { id: number; label: string; done: boolean };

const initialTodoData: Todo[] = [
  { id: 1, label: "Meet Denji at the café", done: true },
  { id: 2, label: "Watch a movie in the rain", done: false },
  { id: 3, label: "Teach Denji to swim", done: false },
  { id: 4, label: "Bite the grenade pin", done: false },
];

function Todos() {
  const context = useContext(ShowcaseContext);
  const todos = store(initialTodoData);
  let failed = $signal<Todo | undefined>(undefined);
  const toggle = $action(async (todo: Todo) => {
    failed = undefined;
    todo.done = !todo.done;
    await sleep(700);
    if (!context.online()) {
      failed = todo;
      throw new Error("no signal");
    }
  });

  return (
    <div class="flex w-full flex-col gap-3 px-6">
      <div class="flex items-center gap-[1ch]">
        <span class="font-medium">Todos</span>
        <Show when={toggle.pending > 0}>
          <span class="animate-pulse opacity-60">(saving)</span>
        </Show>
      </div>
      <ul class="flex flex-col gap-2">
        <For each={todos}>
          {(todo) => (
            <li>
              <label class="flex cursor-default items-center gap-2">
                <input type="checkbox" class="checkbox" checked={todo.done} onChange={() => toggle(todo).catch(() => {})} />
                <span
                  class={[
                    `transition-[color,opacity] duration-150`,
                    todo.done && "line-through opacity-40",
                    failed === todo && "text-red-500",
                  ]}
                >
                  {todo.label}
                </span>
              </label>
            </li>
          )}
        </For>
      </ul>
      <button type="button" class="btn mt-auto" onClick={() => (context.setOnline(prev => !prev))}>
        {context.online() ? "🛜 Online" : "📴 Offline"}
      </button>
    </div>
  );
}

type ListRow = { id: number; text: string };

const ROW_COUNT = 1000;

function buildRows(startId: number, revision: number): ListRow[] {
  return Array.from({ length: ROW_COUNT }, (_, i) => {
    const id = startId + i;
    return { id, text: `Row ${id} · rev ${revision}` };
  });
}

function ThousandRows() {
  let rows = $signal<ListRow[]>([]);
  let lastMs = $signal<number | undefined>(undefined);
  let nextId = 1;
  let revision = 0;

  const measure = (update: () => void) => {
    const start = performance.now();
    update();
    requestAnimationFrame(() => requestAnimationFrame(() => (lastMs = performance.now() - start)));
  };
  const renderRows = () =>
    measure(() => {
      revision += 1;
      const startId = nextId;
      nextId += ROW_COUNT;
      rows = buildRows(startId, revision);
    });
  const rerenderRows = () =>
    measure(() => {
      revision += 1;
      rows = rows.map((row) => ({ ...row, text: `Row ${row.id} · rev ${revision}` }));
    });

  return (
    <div class="flex h-full w-full flex-col gap-3 p-5">
      <div class="flex items-baseline justify-between gap-2">
        <span class="font-medium">1,000 rows</span>
        <output class="text-xs tabular-nums opacity-60">{lastMs === undefined ? "not rendered" : `${lastMs!.toFixed(1)} ms`}</output>
      </div>
      <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain rounded-lg border border-border">
        <For each={rows} keyed={(row) => row.id} fallback={<p class="px-3 py-2 text-sm opacity-60">Press render to fill this list.</p>}>
          {(row) => (
            <div class="flex gap-2 border-b border-border/60 px-3 py-1 text-xs last:border-b-0">
              <span class="tabular-nums opacity-50">{row().id}</span>
              <span class="truncate">{row().text}</span>
            </div>
          )}
        </For>
      </div>
      <div class="flex gap-2">
        <button type="button" class="btn flex-1" onClick={renderRows}>
          Render
        </button>
        <button type="button" class="btn flex-1" onClick={rerenderRows} disabled={rows.length === 0}>
          Re-render
        </button>
      </div>
    </div>
  );
}
