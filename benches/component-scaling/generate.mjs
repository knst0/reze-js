const archetypes = ["StatCard", "DataTable", "FilterForm", "BarChart", "TabsPanel"];

export const frameworks = ["reze", "solid-1", "solid-2", "react-19", "react-19-compiler", "octane", "vue-3.6", "vue-3.6-vapor", "svelte-5"];

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
const sortRows = (list, desc) => `${desc} ? ${list}.sort((a, b) => b.value - a.value) : ${list}.sort((a, b) => a.value - b.value)`;

function signalJsx(flavor) {
  const isReze = flavor === "reze";
  const imports = isReze ? "reze-js" : "solid-js";
  const signal = isReze ? "signal" : "createSignal";
  const computed = isReze ? "computed" : "createMemo";
  const classes = (base, toggles) =>
    flavor === "solid-1"
      ? base
        ? `class="${base}" classList={{ ${toggles} }}`
        : `classList={{ ${toggles} }}`
      : base
        ? `class={["${base}", { ${toggles} }]}`
        : `class={{ ${toggles} }}`;
  const item = (name) => (isReze ? `${name}()` : name);
  const forKey = isReze ? " key={(row) => row.id}" : "";

  return {
    StatCard: (i, w = vocab(i)) => `import { ${computed}, ${signal}, Show } from "${imports}";

export function StatCard${i}(props) {
  const [open, setOpen] = ${signal}(false);
  const [count, setCount] = ${signal}(props.seed);
  const trend = ${computed}(() => (count() > ${i % 50} ? "up" : "down"));
  return (
    <article ${classes(`card stat-${w.slug}`, `up: trend() === "up"`)}>
      <header>
        <h3>{props.title}</h3>
        <button onClick={() => setOpen((v) => !v)}>{open() ? "Hide" : "Show"}</button>
      </header>
      <p class="value">{count()}</p>
      <p class="trend">${w.trend}: {trend()}</p>
      <Show when={open()}>
        <footer>
          <button onClick={() => setCount((n) => n + 1)}>+1</button>
          <button onClick={() => setCount((n) => n - 1)}>-1</button>
        </footer>
      </Show>
    </article>
  );
}
`,
    DataTable: (i, w = vocab(i)) => `import { ${computed}, For, ${signal} } from "${imports}";

export function DataTable${i}(props) {
  const [desc, setDesc] = ${signal}(false);
  const rows = ${computed}(() => {
    const list = ${rowsExpr("props.seed", i)};
    return ${sortRows("list", "desc()")};
  });
  return (
    <section class="table table-${w.slug}">
      <h3>{props.title}</h3>
      <table>
        <thead>
          <tr>
            <th onClick={() => setDesc((v) => !v)}>Name {desc() ? "▼" : "▲"}</th>
            <th>${w.metric}</th>
            <th>Status</th>
          </tr>
        </thead>
        <tbody>
          <For each={rows()}${forKey}>
            {(row) => (
              <tr>
                <td>{${item("row")}.name}</td>
                <td>{${item("row")}.value}</td>
                <td class={${item("row")}.value > 50 ? "ok" : "warn"}>{${item("row")}.value > 50 ? "OK" : "Low"}</td>
              </tr>
            )}
          </For>
        </tbody>
      </table>
    </section>
  );
}
`,
    FilterForm: (i, w = vocab(i)) => `import { ${computed}, ${signal} } from "${imports}";

export function FilterForm${i}(props) {
  const [query, setQuery] = ${signal}("");
  const [category, setCategory] = ${signal}("all");
  const [onlyActive, setOnlyActive] = ${signal}(false);
  const summary = ${computed}(() => category() + ":" + query() + (onlyActive() ? " active" : ""));
  return (
    <form class="filter filter-${w.slug}" onSubmit={(e) => e.preventDefault()}>
      <h3>{props.title}</h3>
      <label>
        ${w.search} <input value={query()} onInput={(e) => setQuery(e.currentTarget.value)} />
      </label>
      <select value={category()} onChange={(e) => setCategory(e.currentTarget.value)}>
        <option value="all">All</option>
${options(w, "        ")}      </select>
      <label>
        <input type="checkbox" checked={onlyActive()} onChange={(e) => setOnlyActive(e.currentTarget.checked)} /> ${w.toggle}
      </label>
      <output>{summary()}</output>
    </form>
  );
}
`,
    BarChart: (i, w = vocab(i)) => `import { For, Show, ${signal} } from "${imports}";

export function BarChart${i}(props) {
  const bars = ${barsExpr("props.seed", i)};
  const [hovered, setHovered] = ${signal}(-1);
  return (
    <figure class="chart chart-${w.slug}">
      <figcaption>{props.title}</figcaption>
      <div class="bars">
        <For each={bars}>
          {(bar, k) => (
            <div
              ${classes("bar", "active: hovered() === k()")}
              style={{ height: ${item("bar")}.value + "%" }}
              onMouseEnter={() => setHovered(k())}
              onMouseLeave={() => setHovered(-1)}
            />
          )}
        </For>
      </div>
      <Show when={hovered() >= 0} fallback={<p class="tip muted">${w.hint}</p>}>
        <p class="tip">
          {bars[hovered()].label}: {bars[hovered()].value}
        </p>
      </Show>
    </figure>
  );
}
`,
    TabsPanel: (i, w = vocab(i)) => `import { Match, ${signal}, Switch } from "${imports}";

export function TabsPanel${i}(props) {
  const [tab, setTab] = ${signal}("overview");
  return (
    <div class="tabs tabs-${w.slug}">
      <nav>
        <button ${classes("", `active: tab() === "overview"`)} onClick={() => setTab("overview")}>${w.tabs[0]}</button>
        <button ${classes("", `active: tab() === "details"`)} onClick={() => setTab("details")}>${w.tabs[1]}</button>
        <button ${classes("", `active: tab() === "logs"`)} onClick={() => setTab("logs")}>${w.tabs[2]}</button>
      </nav>
      <Switch
        fallback={
          <ul>
            <li>${w.log1}</li>
            <li>${w.log2}</li>
          </ul>
        }
      >
        <Match when={tab() === "overview"}>
          <p>${w.overview} {props.title}</p>
        </Match>
        <Match when={tab() === "details"}>
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
}

const react = {
  StatCard: (i, w = vocab(i)) => `import { useState } from "react";

export function StatCard${i}({ title, seed }) {
  const [open, setOpen] = useState(false);
  const [count, setCount] = useState(seed);
  const trend = count > ${i % 50} ? "up" : "down";
  return (
    <article className={"card stat-${w.slug}" + (trend === "up" ? " up" : "")}>
      <header>
        <h3>{title}</h3>
        <button onClick={() => setOpen((v) => !v)}>{open ? "Hide" : "Show"}</button>
      </header>
      <p className="value">{count}</p>
      <p className="trend">${w.trend}: {trend}</p>
      {open && (
        <footer>
          <button onClick={() => setCount((n) => n + 1)}>+1</button>
          <button onClick={() => setCount((n) => n - 1)}>-1</button>
        </footer>
      )}
    </article>
  );
}
`,
  DataTable: (i, w = vocab(i)) => `import { useMemo, useState } from "react";

export function DataTable${i}({ title, seed }) {
  const [desc, setDesc] = useState(false);
  const rows = useMemo(() => {
    const list = ${rowsExpr("seed", i)};
    return ${sortRows("list", "desc")};
  }, [seed, desc]);
  return (
    <section className="table table-${w.slug}">
      <h3>{title}</h3>
      <table>
        <thead>
          <tr>
            <th onClick={() => setDesc((v) => !v)}>Name {desc ? "▼" : "▲"}</th>
            <th>${w.metric}</th>
            <th>Status</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.id}>
              <td>{row.name}</td>
              <td>{row.value}</td>
              <td className={row.value > 50 ? "ok" : "warn"}>{row.value > 50 ? "OK" : "Low"}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}
`,
  FilterForm: (i, w = vocab(i)) => `import { useState } from "react";

export function FilterForm${i}({ title }) {
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState("all");
  const [onlyActive, setOnlyActive] = useState(false);
  const summary = category + ":" + query + (onlyActive ? " active" : "");
  return (
    <form className="filter filter-${w.slug}" onSubmit={(e) => e.preventDefault()}>
      <h3>{title}</h3>
      <label>
        ${w.search} <input value={query} onChange={(e) => setQuery(e.currentTarget.value)} />
      </label>
      <select value={category} onChange={(e) => setCategory(e.currentTarget.value)}>
        <option value="all">All</option>
${options(w, "        ")}      </select>
      <label>
        <input type="checkbox" checked={onlyActive} onChange={(e) => setOnlyActive(e.currentTarget.checked)} /> ${w.toggle}
      </label>
      <output>{summary}</output>
    </form>
  );
}
`,
  BarChart: (i, w = vocab(i)) => `import { useMemo, useState } from "react";

export function BarChart${i}({ title, seed }) {
  const bars = useMemo(() => ${barsExpr("seed", i)}, [seed]);
  const [hovered, setHovered] = useState(-1);
  return (
    <figure className="chart chart-${w.slug}">
      <figcaption>{title}</figcaption>
      <div className="bars">
        {bars.map((bar, k) => (
          <div
            key={k}
            className={"bar" + (hovered === k ? " active" : "")}
            style={{ height: bar.value + "%" }}
            onMouseEnter={() => setHovered(k)}
            onMouseLeave={() => setHovered(-1)}
          />
        ))}
      </div>
      {hovered >= 0 ? (
        <p className="tip">
          {bars[hovered].label}: {bars[hovered].value}
        </p>
      ) : (
        <p className="tip muted">${w.hint}</p>
      )}
    </figure>
  );
}
`,
  TabsPanel: (i, w = vocab(i)) => `import { useState } from "react";

export function TabsPanel${i}({ title, seed }) {
  const [tab, setTab] = useState("overview");
  return (
    <div className="tabs tabs-${w.slug}">
      <nav>
        <button className={tab === "overview" ? "active" : ""} onClick={() => setTab("overview")}>${w.tabs[0]}</button>
        <button className={tab === "details" ? "active" : ""} onClick={() => setTab("details")}>${w.tabs[1]}</button>
        <button className={tab === "logs" ? "active" : ""} onClick={() => setTab("logs")}>${w.tabs[2]}</button>
      </nav>
      {tab === "overview" ? (
        <p>${w.overview} {title}</p>
      ) : tab === "details" ? (
        <dl>
          <dt>${w.seedLabel}</dt>
          <dd>{seed}</dd>
          <dt>${w.idLabel}</dt>
          <dd>${i}</dd>
        </dl>
      ) : (
        <ul>
          <li>${w.log1}</li>
          <li>${w.log2}</li>
        </ul>
      )}
    </div>
  );
}
`,
};

const octane = {
  StatCard: (i, w = vocab(i)) => `import { useState } from "octane";

export function StatCard${i}({ title, seed }: { title: string; seed: number }) @{
  const [open, setOpen] = useState(false);
  const [count, setCount] = useState(seed);
  const trend = count > ${i % 50} ? "up" : "down";
  <article class={["card stat-${w.slug}", { up: trend === "up" }]}>
    <header>
      <h3>{title as string}</h3>
      <button onClick={() => setOpen((v) => !v)}>{(open ? "Hide" : "Show") as string}</button>
    </header>
    <p class="value">{String(count)}</p>
    <p class="trend">${w.trend}: {trend as string}</p>
    @if (open) {
      <footer>
        <button onClick={() => setCount((n) => n + 1)}>+1</button>
        <button onClick={() => setCount((n) => n - 1)}>-1</button>
      </footer>
    }
  </article>
}
`,
  DataTable: (i, w = vocab(i)) => `import { useMemo, useState } from "octane";

export function DataTable${i}({ title, seed }: { title: string; seed: number }) @{
  const [desc, setDesc] = useState(false);
  const rows = useMemo(() => {
    const list = ${rowsExpr("seed", i)};
    return ${sortRows("list", "desc")};
  });
  <section class="table table-${w.slug}">
    <h3>{title as string}</h3>
    <table>
      <thead>
        <tr>
          <th onClick={() => setDesc((v) => !v)}>Name {(desc ? "▼" : "▲") as string}</th>
          <th>${w.metric}</th>
          <th>Status</th>
        </tr>
      </thead>
      <tbody>
        @for (const row of rows; key row.id) {
          <tr>
            <td>{row.name as string}</td>
            <td>{String(row.value)}</td>
            <td class={row.value > 50 ? "ok" : "warn"}>{(row.value > 50 ? "OK" : "Low") as string}</td>
          </tr>
        }
      </tbody>
    </table>
  </section>
}
`,
  FilterForm: (i, w = vocab(i)) => `import { useState } from "octane";

export function FilterForm${i}({ title }: { title: string }) @{
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState("all");
  const [onlyActive, setOnlyActive] = useState(false);
  const summary = category + ":" + query + (onlyActive ? " active" : "");
  <form class="filter filter-${w.slug}" onSubmit={(e) => e.preventDefault()}>
    <h3>{title as string}</h3>
    <label>
      ${w.search} <input value={query} onInput={(e) => setQuery(e.currentTarget.value)} />
    </label>
    <select value={category} onChange={(e) => setCategory(e.currentTarget.value)}>
      <option value="all">All</option>
${options(w, "      ")}    </select>
    <label>
      <input type="checkbox" checked={onlyActive} onChange={(e) => setOnlyActive(e.currentTarget.checked)} /> ${w.toggle}
    </label>
    <output>{summary}</output>
  </form>
}
`,
  BarChart: (i, w = vocab(i)) => `import { useMemo, useState } from "octane";

export function BarChart${i}({ title, seed }: { title: string; seed: number }) @{
  const bars = useMemo(() => ${barsExpr("seed", i)});
  const [hovered, setHovered] = useState<string | null>(null);
  const active = bars.find((bar) => bar.label === hovered);
  <figure class="chart chart-${w.slug}">
    <figcaption>{title as string}</figcaption>
    <div class="bars">
      @for (const bar of bars; key bar.label) {
        <div
          class={["bar", { active: hovered === bar.label }]}
          style={{ height: bar.value + "%" }}
          onMouseEnter={() => setHovered(bar.label)}
          onMouseLeave={() => setHovered(null)}
        />
      }
    </div>
    @if (active) {
      <p class="tip">{active.label + ": " + active.value}</p>
    } @else {
      <p class="tip muted">${w.hint}</p>
    }
  </figure>
}
`,
  TabsPanel: (i, w = vocab(i)) => `import { useState } from "octane";

export function TabsPanel${i}({ title, seed }: { title: string; seed: number }) @{
  const [tab, setTab] = useState("overview");
  <div class="tabs tabs-${w.slug}">
    <nav>
      <button class={{ active: tab === "overview" }} onClick={() => setTab("overview")}>${w.tabs[0]}</button>
      <button class={{ active: tab === "details" }} onClick={() => setTab("details")}>${w.tabs[1]}</button>
      <button class={{ active: tab === "logs" }} onClick={() => setTab("logs")}>${w.tabs[2]}</button>
    </nav>
    @switch (tab) {
      @case "overview": {
        <p>${w.overview} {title as string}</p>
      }
      @case "details": {
        <dl>
          <dt>${w.seedLabel}</dt>
          <dd>{String(seed)}</dd>
          <dt>${w.idLabel}</dt>
          <dd>${i}</dd>
        </dl>
      }
      @default: {
        <ul>
          <li>${w.log1}</li>
          <li>${w.log2}</li>
        </ul>
      }
    }
  </div>
}
`,
};

const vueScript = (vapor, body) => `<script setup${vapor ? " vapor" : ""}>
${body.trim()}
</script>
`;

const vue = (vapor) => ({
  StatCard: (i, w = vocab(i)) =>
    vueScript(
      vapor,
      `
import { computed, ref } from "vue";
const props = defineProps({ title: String, seed: Number });
const open = ref(false);
const count = ref(props.seed);
const trend = computed(() => (count.value > ${i % 50} ? "up" : "down"));
`,
    ) +
    `
<template>
  <article class="card stat-${w.slug}" :class="{ up: trend === 'up' }">
    <header>
      <h3>{{ title }}</h3>
      <button @click="open = !open">{{ open ? "Hide" : "Show" }}</button>
    </header>
    <p class="value">{{ count }}</p>
    <p class="trend">${w.trend}: {{ trend }}</p>
    <footer v-if="open">
      <button @click="count++">+1</button>
      <button @click="count--">-1</button>
    </footer>
  </article>
</template>
`,
  DataTable: (i, w = vocab(i)) =>
    vueScript(
      vapor,
      `
import { computed, ref } from "vue";
const props = defineProps({ title: String, seed: Number });
const desc = ref(false);
const rows = computed(() => {
  const list = ${rowsExpr("props.seed", i)};
  return ${sortRows("list", "desc.value")};
});
`,
    ) +
    `
<template>
  <section class="table table-${w.slug}">
    <h3>{{ title }}</h3>
    <table>
      <thead>
        <tr>
          <th @click="desc = !desc">Name {{ desc ? "▼" : "▲" }}</th>
          <th>${w.metric}</th>
          <th>Status</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="row in rows" :key="row.id">
          <td>{{ row.name }}</td>
          <td>{{ row.value }}</td>
          <td :class="row.value > 50 ? 'ok' : 'warn'">{{ row.value > 50 ? "OK" : "Low" }}</td>
        </tr>
      </tbody>
    </table>
  </section>
</template>
`,
  FilterForm: (i, w = vocab(i)) =>
    vueScript(
      vapor,
      `
import { computed, ref } from "vue";
defineProps({ title: String, seed: Number });
const query = ref("");
const category = ref("all");
const onlyActive = ref(false);
const summary = computed(() => category.value + ":" + query.value + (onlyActive.value ? " active" : ""));
`,
    ) +
    `
<template>
  <form class="filter filter-${w.slug}" @submit.prevent>
    <h3>{{ title }}</h3>
    <label>${w.search} <input v-model="query" /></label>
    <select v-model="category">
      <option value="all">All</option>
${options(w, "      ")}    </select>
    <label><input v-model="onlyActive" type="checkbox" /> ${w.toggle}</label>
    <output>{{ summary }}</output>
  </form>
</template>
`,
  BarChart: (i, w = vocab(i)) =>
    vueScript(
      vapor,
      `
import { ref } from "vue";
const props = defineProps({ title: String, seed: Number });
const bars = ${barsExpr("props.seed", i)};
const hovered = ref(-1);
`,
    ) +
    `
<template>
  <figure class="chart chart-${w.slug}">
    <figcaption>{{ title }}</figcaption>
    <div class="bars">
      <div
        v-for="(bar, k) in bars"
        :key="k"
        class="bar"
        :class="{ active: hovered === k }"
        :style="{ height: bar.value + '%' }"
        @mouseenter="hovered = k"
        @mouseleave="hovered = -1"
      />
    </div>
    <p v-if="hovered >= 0" class="tip">{{ bars[hovered].label }}: {{ bars[hovered].value }}</p>
    <p v-else class="tip muted">${w.hint}</p>
  </figure>
</template>
`,
  TabsPanel: (i, w = vocab(i)) =>
    vueScript(
      vapor,
      `
import { ref } from "vue";
defineProps({ title: String, seed: Number });
const tab = ref("overview");
`,
    ) +
    `
<template>
  <div class="tabs tabs-${w.slug}">
    <nav>
      <button :class="{ active: tab === 'overview' }" @click="tab = 'overview'">${w.tabs[0]}</button>
      <button :class="{ active: tab === 'details' }" @click="tab = 'details'">${w.tabs[1]}</button>
      <button :class="{ active: tab === 'logs' }" @click="tab = 'logs'">${w.tabs[2]}</button>
    </nav>
    <p v-if="tab === 'overview'">${w.overview} {{ title }}</p>
    <dl v-else-if="tab === 'details'">
      <dt>${w.seedLabel}</dt>
      <dd>{{ seed }}</dd>
      <dt>${w.idLabel}</dt>
      <dd>${i}</dd>
    </dl>
    <ul v-else>
      <li>${w.log1}</li>
      <li>${w.log2}</li>
    </ul>
  </div>
</template>
`,
});

const svelte = {
  StatCard: (i, w = vocab(i)) => `<script>
  let { title, seed } = $props();
  let open = $state(false);
  let count = $state(seed);
  const trend = $derived(count > ${i % 50} ? "up" : "down");
</script>

<article class="card stat-${w.slug}" class:up={trend === "up"}>
  <header>
    <h3>{title}</h3>
    <button onclick={() => (open = !open)}>{open ? "Hide" : "Show"}</button>
  </header>
  <p class="value">{count}</p>
  <p class="trend">${w.trend}: {trend}</p>
  {#if open}
    <footer>
      <button onclick={() => count++}>+1</button>
      <button onclick={() => count--}>-1</button>
    </footer>
  {/if}
</article>
`,
  DataTable: (i, w = vocab(i)) => `<script>
  let { title, seed } = $props();
  let desc = $state(false);
  const rows = $derived.by(() => {
    const list = ${rowsExpr("seed", i)};
    return ${sortRows("list", "desc")};
  });
</script>

<section class="table table-${w.slug}">
  <h3>{title}</h3>
  <table>
    <thead>
      <tr>
        <th onclick={() => (desc = !desc)}>Name {desc ? "▼" : "▲"}</th>
        <th>${w.metric}</th>
        <th>Status</th>
      </tr>
    </thead>
    <tbody>
      {#each rows as row (row.id)}
        <tr>
          <td>{row.name}</td>
          <td>{row.value}</td>
          <td class={row.value > 50 ? "ok" : "warn"}>{row.value > 50 ? "OK" : "Low"}</td>
        </tr>
      {/each}
    </tbody>
  </table>
</section>
`,
  FilterForm: (i, w = vocab(i)) => `<script>
  let { title } = $props();
  let query = $state("");
  let category = $state("all");
  let onlyActive = $state(false);
  const summary = $derived(category + ":" + query + (onlyActive ? " active" : ""));
</script>

<form class="filter filter-${w.slug}" onsubmit={(e) => e.preventDefault()}>
  <h3>{title}</h3>
  <label>${w.search} <input bind:value={query} /></label>
  <select bind:value={category}>
    <option value="all">All</option>
${options(w, "    ")}  </select>
  <label><input type="checkbox" bind:checked={onlyActive} /> ${w.toggle}</label>
  <output>{summary}</output>
</form>
`,
  BarChart: (i, w = vocab(i)) => `<script>
  let { title, seed } = $props();
  const bars = ${barsExpr("seed", i)};
  let hovered = $state(-1);
</script>

<figure class="chart chart-${w.slug}">
  <figcaption>{title}</figcaption>
  <div class="bars">
    {#each bars as bar, k}
      <div
        class="bar"
        class:active={hovered === k}
        style:height={bar.value + "%"}
        onmouseenter={() => (hovered = k)}
        onmouseleave={() => (hovered = -1)}
      ></div>
    {/each}
  </div>
  {#if hovered >= 0}
    <p class="tip">{bars[hovered].label}: {bars[hovered].value}</p>
  {:else}
    <p class="tip muted">${w.hint}</p>
  {/if}
</figure>
`,
  TabsPanel: (i, w = vocab(i)) => `<script>
  let { title, seed } = $props();
  let tab = $state("overview");
</script>

<div class="tabs tabs-${w.slug}">
  <nav>
    <button class:active={tab === "overview"} onclick={() => (tab = "overview")}>${w.tabs[0]}</button>
    <button class:active={tab === "details"} onclick={() => (tab = "details")}>${w.tabs[1]}</button>
    <button class:active={tab === "logs"} onclick={() => (tab = "logs")}>${w.tabs[2]}</button>
  </nav>
  {#if tab === "overview"}
    <p>${w.overview} {title}</p>
  {:else if tab === "details"}
    <dl>
      <dt>${w.seedLabel}</dt>
      <dd>{seed}</dd>
      <dt>${w.idLabel}</dt>
      <dd>${i}</dd>
    </dl>
  {:else}
    <ul>
      <li>${w.log1}</li>
      <li>${w.log2}</li>
    </ul>
  {/if}
</div>
`,
};

const html = (entry) => `<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <title>Dashboard</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/${entry}"></script>
  </body>
</html>
`;

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

function jsxApp(list, { classAttr, importExt = "" }) {
  const imports = list.map((w) => `import { ${w.name} } from "./${w.name}${importExt}";\n`).join("");
  const body = list.flatMap((w) => w.uses.map((u) => `      <${w.name} title="${u.title}" seed={${u.seed}} />\n`)).join("");
  return `${imports}
export function App() {
  return (
    <main ${classAttr}="dashboard">
      <h1>Dashboard</h1>
${body}    </main>
  );
}
`;
}

const jsxMains = {
  reze: `import { render } from "reze-js";

import { App } from "./App";

render(() => <App />, document.getElementById("app"));
`,
  "solid-1": `import { render } from "solid-js/web";

import { App } from "./App";

render(() => <App />, document.getElementById("app"));
`,
  "solid-2": `import { render } from "@solidjs/web";

import { App } from "./App";

render(() => <App />, document.getElementById("app"));
`,
};

/**
 * Builds a dashboard app of `components` distinct widget components, each rendered `usages` times.
 * Returns a map of paths relative to the framework's `src` directory to file contents.
 */
export function generate(framework, { components, usages }) {
  const list = widgets(components, usages);
  const files = {};
  if (framework === "reze" || framework === "solid-1" || framework === "solid-2") {
    const sources = signalJsx(framework);
    for (const w of list) files[`${w.name}.jsx`] = sources[w.archetype](w.i);
    files["App.jsx"] = jsxApp(list, { classAttr: "class" });
    files["main.jsx"] = jsxMains[framework];
    files["index.html"] = html("main.jsx");
  } else if (framework === "react-19" || framework === "react-19-compiler") {
    for (const w of list) files[`${w.name}.jsx`] = react[w.archetype](w.i);
    files["App.jsx"] = jsxApp(list, { classAttr: "className" });
    files["main.jsx"] = `import { createRoot } from "react-dom/client";

import { App } from "./App";

createRoot(document.getElementById("app")).render(<App />);
`;
    files["index.html"] = html("main.jsx");
  } else if (framework === "octane") {
    for (const w of list) files[`${w.name}.tsrx`] = octane[w.archetype](w.i);
    const imports = list.map((w) => `import { ${w.name} } from "./${w.name}.tsrx";\n`).join("");
    const body = list.flatMap((w) => w.uses.map((u) => `    <${w.name} title="${u.title}" seed={${u.seed}} />\n`)).join("");
    files["App.tsrx"] = `${imports}
export function App() @{
  <main class="dashboard">
    <h1>Dashboard</h1>
${body}  </main>
}
`;
    files["main.js"] = `import { createRoot } from "octane";

import { App } from "./App.tsrx";

createRoot(document.getElementById("app")).render(App);
`;
    files["index.html"] = html("main.js");
  } else if (framework === "vue-3.6" || framework === "vue-3.6-vapor") {
    const vapor = framework === "vue-3.6-vapor";
    const sources = vue(vapor);
    for (const w of list) files[`${w.name}.vue`] = sources[w.archetype](w.i);
    const imports = list.map((w) => `import ${w.name} from "./${w.name}.vue";`).join("\n");
    const body = list.flatMap((w) => w.uses.map((u) => `    <${w.name} title="${u.title}" :seed="${u.seed}" />\n`)).join("");
    files["App.vue"] =
      vueScript(vapor, imports) +
      `
<template>
  <main class="dashboard">
    <h1>Dashboard</h1>
${body}  </main>
</template>
`;
    files["main.js"] = vapor
      ? `import { createVaporApp } from "vue";

import App from "./App.vue";

createVaporApp(App).mount("#app");
`
      : `import { createApp } from "vue";

import App from "./App.vue";

createApp(App).mount("#app");
`;
    files["index.html"] = html("main.js");
  } else if (framework === "svelte-5") {
    for (const w of list) files[`${w.name}.svelte`] = svelte[w.archetype](w.i);
    const imports = list.map((w) => `  import ${w.name} from "./${w.name}.svelte";\n`).join("");
    const body = list.flatMap((w) => w.uses.map((u) => `  <${w.name} title="${u.title}" seed={${u.seed}} />\n`)).join("");
    files["App.svelte"] = `<script>
${imports}</script>

<main class="dashboard">
  <h1>Dashboard</h1>
${body}</main>
`;
    files["main.js"] = `import { mount } from "svelte";

import App from "./App.svelte";

mount(App, { target: document.getElementById("app") });
`;
    files["index.html"] = html("main.js");
  } else {
    throw new Error(`unknown framework ${framework}`);
  }
  return files;
}
