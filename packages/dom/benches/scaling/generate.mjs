const archetypes = ["StatCard", "DataTable", "FilterForm", "BarChart", "TabsPanel"];

const syllables = ["ka", "lo", "mi", "ren", "tas", "vu", "dor", "pel", "zi", "qua", "bri", "nox", "sel", "fa", "gim", "hu"];

function word(next, capital) {
  let out = "";
  const length = 2 + (next() % 2);
  for (let k = 0; k < length; k++) out += syllables[next() % syllables.length];
  return capital ? out[0].toUpperCase() + out.slice(1) : out;
}

function vocab(i) {
  let state = (i + 1) * 2654435761;
  const next = () => {
    state = (Math.imul(state ^ (state >>> 15), 2246822507) + 0x9e3779b9) >>> 0;
    return state >>> 8;
  };
  const phrase = (count) => Array.from({ length: count }, (_, k) => word(next, k === 0)).join(" ");
  return {
    slug: `${word(next)}-${i}`,
    metric: phrase(2),
    search: phrase(1),
    item: phrase(1),
    bar: word(next, true),
    log1: phrase(3),
    log2: phrase(3),
    hint: phrase(3),
    toggle: phrase(2),
    trend: phrase(1),
    overview: phrase(2),
    tabs: [phrase(1), phrase(1), phrase(1)],
    seedLabel: phrase(1),
    idLabel: phrase(1),
    options: Array.from({ length: 2 + (next() % 4) }, () => word(next, true)),
  };
}

const options = (w, indent) => w.options.map((label) => `${indent}<option value="${label.toLowerCase()}">${label}</option>\n`).join("");

const rowsExpr = (seed, i, w = vocab(i)) =>
  `Array.from({ length: 6 }, (_, k) => ({ id: k, name: "${w.item} " + k, value: (${seed} * (k + 3) + ${i}) % 100 }))`;
const barsExpr = (seed, i, w = vocab(i)) =>
  `Array.from({ length: 8 }, (_, k) => ({ label: "${w.bar} " + (k + 1), value: (${seed} * (k + 5) + ${i}) % 100 }))`;

const templates = {
  StatCard: (i, w = vocab(i)) => `import { Show, computed, signal } from "reze-js";

export function StatCard${i}(props) {
  let open = signal(false);
  let count = signal(props.seed);
  const trend = computed(count > ${i % 50} ? "up" : "down");
  return (
    <article class={["card stat-${w.slug}", { up: trend === "up" }]}>
      <header>
        <h3>{props.title}</h3>
        <button onClick={() => (open = !open)}>{open ? "Hide" : "Show"}</button>
      </header>
      <p class="value">{count}</p>
      <p class="trend">${w.trend}: {trend}</p>
      <Show when={open}>
        <footer>
          <button onClick={() => (count += 1)}>+1</button>
          <button onClick={() => (count -= 1)}>-1</button>
        </footer>
      </Show>
    </article>
  );
}
`,
  DataTable: (i, w = vocab(i)) => `import { For, computed, signal } from "reze-js";

export function DataTable${i}(props) {
  let desc = signal(false);
  const rows = computed(
    ${rowsExpr("props.seed", i)}.sort((a, b) => (desc ? b.value - a.value : a.value - b.value)),
  );
  return (
    <section class="table table-${w.slug}">
      <h3>{props.title}</h3>
      <table>
        <thead>
          <tr>
            <th onClick={() => (desc = !desc)}>Name {desc ? "▼" : "▲"}</th>
            <th>${w.metric}</th>
            <th>Status</th>
          </tr>
        </thead>
        <tbody>
          <For each={rows} keyed={(row) => row.id}>
            {(row) => (
              <tr>
                <td>{row.name}</td>
                <td>{row.value}</td>
                <td class={row.value > 50 ? "ok" : "warn"}>{row.value > 50 ? "OK" : "Low"}</td>
              </tr>
            )}
          </For>
        </tbody>
      </table>
    </section>
  );
}
`,
  FilterForm: (i, w = vocab(i)) => `import { computed, signal } from "reze-js";

export function FilterForm${i}(props) {
  let query = signal("");
  let category = signal("all");
  let onlyActive = signal(false);
  const summary = computed(category + ":" + query + (onlyActive ? " active" : ""));
  return (
    <form class="filter filter-${w.slug}" onSubmit={(e) => e.preventDefault()}>
      <h3>{props.title}</h3>
      <label>
        ${w.search} <input value={query} onInput={(e) => (query = e.currentTarget.value)} />
      </label>
      <select value={category} onChange={(e) => (category = e.currentTarget.value)}>
        <option value="all">All</option>
${options(w, "        ")}      </select>
      <label>
        <input type="checkbox" checked={onlyActive} onChange={(e) => (onlyActive = e.currentTarget.checked)} /> ${w.toggle}
      </label>
      <output>{summary}</output>
    </form>
  );
}
`,
  BarChart: (i, w = vocab(i)) => `import { For, Show, signal } from "reze-js";

export function BarChart${i}(props) {
  const bars = ${barsExpr("props.seed", i)};
  let hovered = signal(-1);
  return (
    <figure class="chart chart-${w.slug}">
      <figcaption>{props.title}</figcaption>
      <div class="bars">
        <For each={bars}>
          {(bar, k) => (
            <div
              class={["bar", { active: hovered === k }]}
              style={{ height: bar.value + "%" }}
              onMouseEnter={() => (hovered = k)}
              onMouseLeave={() => (hovered = -1)}
            />
          )}
        </For>
      </div>
      <Show when={hovered >= 0} fallback={<p class="tip muted">${w.hint}</p>}>
        <p class="tip">
          {bars[hovered].label}: {bars[hovered].value}
        </p>
      </Show>
    </figure>
  );
}
`,
  TabsPanel: (i, w = vocab(i)) => `import { Match, Switch, signal } from "reze-js";

export function TabsPanel${i}(props) {
  let tab = signal("overview");
  return (
    <div class="tabs tabs-${w.slug}">
      <nav>
        <button class={{ active: tab === "overview" }} onClick={() => (tab = "overview")}>${w.tabs[0]}</button>
        <button class={{ active: tab === "details" }} onClick={() => (tab = "details")}>${w.tabs[1]}</button>
        <button class={{ active: tab === "logs" }} onClick={() => (tab = "logs")}>${w.tabs[2]}</button>
      </nav>
      <Switch
        fallback={
          <ul>
            <li>${w.log1}</li>
            <li>${w.log2}</li>
          </ul>
        }
      >
        <Match when={tab === "overview"}>
          <p>${w.overview} {props.title}</p>
        </Match>
        <Match when={tab === "details"}>
          <dl>
            <dt>${w.seedLabel}</dt>
            <dd>{props.seed}</dd>
            <dt>${w.idLabel}</dt>
            <dd>${i}</dd>
          </dl>
        </Match>
      </Switch>
    </div>
  );
}
`,
};

function widgets(components, usages) {
  const list = [];
  for (let i = 0; i < components; i++) {
    const name = `${archetypes[i % archetypes.length]}${i}`;
    const uses = [];
    for (let u = 0; u < usages; u++) uses.push({ title: `Widget ${i}.${u}`, seed: i * 7 + u });
    list.push({ i, name, archetype: archetypes[i % archetypes.length], uses });
  }
  return list;
}

function app(list) {
  const imports = list.map((w) => `import { ${w.name} } from "./${w.name}";\n`).join("");
  const body = list.flatMap((w) => w.uses.map((u) => `      <${w.name} title="${u.title}" seed={${u.seed}} />\n`)).join("");
  return `${imports}
export function App() {
  return (
    <main class="dashboard">
      <h1>Dashboard</h1>
${body}    </main>
  );
}
`;
}

const main = `import { render } from "reze-js";

import { App } from "./App";

render(() => <App />, document.getElementById("app"));
`;

/**
 * Builds a dashboard of `components` distinct widget components, each rendered `usages` times.
 * Returns a map of file names to contents for a flat source directory.
 */
export function generate({ components, usages }) {
  const list = widgets(components, usages);
  const files = { "App.jsx": app(list), "main.jsx": main };
  for (const w of list) files[`${w.name}.jsx`] = templates[w.archetype](w.i);
  return files;
}
