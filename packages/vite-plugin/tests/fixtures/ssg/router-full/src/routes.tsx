import { defineRoute, defineRoutes, useNavigate, type RouteProps } from "@rezejs/router";
import { computed, effect, signal, type JSX } from "reze-js";
import { asyncComponent } from "reze-js/internal/runtime";

import { logoUrl, inlineLogo, emittedLogo, metaLogo, inlineMetaLogo } from "./assets";
import { readHits } from "./counter-state";

function Home() {
  let n = signal(0);
  return (
    <article>
      <h1 id="home-title">home</h1>
      <p id="home-data">home-data</p>
      <button id="home-inc" type="button" onClick={() => (n += 1)}>
        {n}
      </button>
      <a id="to-about" href="/about">
        about
      </a>
      <a id="to-blog-a" href="/blog/a">
        post a
      </a>
      <a id="to-lazy" href="/lazy">
        lazy
      </a>
    </article>
  );
}

function About() {
  return (
    <article>
      <h1 id="about-title">about</h1>
    </article>
  );
}

function PostPage(props: RouteProps<{ id: string }, { title: string }>) {
  return (
    <article id="post" ref={(node) => node.setAttribute("data-hydrated", "")}>
      <h1 id="post-title">{props.data.title}</h1>
      <p id="post-id">{props.params.id}</p>
    </article>
  );
}

function OptPage(props: RouteProps<{ id: string }, { id: string }>) {
  return <p id="opt-id">{props.params.id ?? "none"}</p>;
}

function FilesPage(props: RouteProps<{ rest: string }, unknown>) {
  return <p id="rest">{props.params.rest}</p>;
}

function DocsLayout(props: { children: JSX.Element }) {
  return (
    <section>
      <h2 id="docs-title">docs</h2>
      {props.children}
    </section>
  );
}

function CounterPage() {
  let n = signal(2);
  let double = computed(n * 2);
  return (
    <article>
      <p id="counter-out">
        {n}:{double}
      </p>
      <button id="counter-inc" type="button" onClick={() => (n += 1)}>
        inc
      </button>
    </article>
  );
}

function EffectPage() {
  let a = signal(2);
  let b = signal(7);
  let doubledA = computed(a * 2);
  let doubledB = computed(b * 2);
  let logged = signal("");
  effect(() => {
    logged = `${a}/${b}`;
  });
  return (
    <article>
      <p id="effect-derived">
        {doubledA}:{doubledB}
      </p>
      <p id="effect-log">{logged}</p>
      <button id="effect-inc" type="button" onClick={() => ((a += 1), (b += 1))}>
        inc
      </button>
    </article>
  );
}

interface AliasData {
  items: string[];
}

function AliasView(props: { data: AliasData }) {
  return asyncComponent(
    async (): Promise<[AliasData, AliasData]> => {
      const first = await Promise.resolve(props.data);
      first.items.push("n");
      const second = await Promise.resolve(props.data);
      return [first, second];
    },
    (values) => {
      const [first, second] = values();
      return (
        <p id="alias">
          {first === second ? "same" : "diff"}:{second.items.join(",")}
        </p>
      );
    },
  );
}

function AliasPage(props: RouteProps<Record<string, string>, { items: string[] }>) {
  return <AliasView data={props.data} />;
}

function NoDataPage(props: RouteProps<Record<string, string>, unknown>) {
  return <p id="nodata">{props.data === undefined ? "no data" : "has data"}</p>;
}

function UndefPage(props: RouteProps<Record<string, string>, undefined>) {
  return <p id="undefdata">{props.data === undefined ? "undef" : "has data"}</p>;
}

function CodecPage(props: RouteProps<Record<string, string>, CodecData>) {
  const data = props.data;
  const errCause: unknown = data.err instanceof Error && "cause" in data.err ? data.err.cause : undefined;
  const checks: Array<[string, boolean]> = [
    ["undef", data.u === undefined],
    ["null", data.nil === null],
    ["bool", data.t === true && data.f === false],
    ["finite", data.int === 42],
    ["negzero", Object.is(data.neg, -0)],
    ["nan", Number.isNaN(data.nan)],
    ["inf", data.inf === Infinity && data.ninf === -Infinity],
    ["bigint", typeof data.big === "bigint" && data.big === 123456789012345678901234567890n],
    ["date", data.date instanceof Date && data.date.toISOString() === "2026-10-04T00:00:00.000Z"],
    ["array", Array.isArray(data.arr) && data.arr.join("|") === "1|two|"],
    ["record", data.rec.nested.b === 2],
    ["error", data.err instanceof Error && data.err.message === "boom" && errCause === "root"],
    ["alias", data.aliasA === data.aliasB && data.aliasA.n === 1],
    ["cycle", data.cyclic.self === data.cyclic],
    ["proto", Object.getPrototypeOf(data.proto) === null && data.proto["__proto__"] === "kept"],
  ];
  return (
    <article id="codec" ref={(node) => node.setAttribute("data-hydrated", "")}>
      {checks.map(([name, ok]) => (
        <p id={`codec-${name}`}>{ok ? "ok" : "FAIL"}</p>
      ))}
      <p id="codec-markup">{data.markup}</p>
    </article>
  );
}

function OnePage() {
  return <p id="hits">{readHits()}</p>;
}

function TwoPage() {
  return <p id="hits">{readHits()}</p>;
}

function AdminPage() {
  useNavigate()("/about", { replace: true });
  return <p>admin</p>;
}

interface CodecData {
  markup: string;
  u: undefined;
  nil: null;
  t: boolean;
  f: boolean;
  int: number;
  neg: number;
  nan: number;
  inf: number;
  ninf: number;
  big: bigint;
  date: Date;
  arr: Array<number | string | null>;
  rec: { a: number; nested: { b: number } };
  err: Error;
  aliasA: { n: number };
  aliasB: { n: number };
  cyclic: { name: string; self?: unknown };
  proto: Record<string, unknown>;
}

function IntroPage() {
  return <p id="intro">intro</p>;
}

function ApiPage() {
  return <p id="api">api</p>;
}

function CondPage() {
  return <p>cond</p>;
}

function AssetsPage() {
  return (
    <article>
      <img id="asset-img" src={logoUrl} alt="asset" />
      <img id="asset-inline" src={inlineLogo} alt="inline" />
      <img id="asset-emitted" src={emittedLogo} alt="emitted" />
      <img id="asset-meta" src={metaLogo} alt="meta" />
      <img id="asset-inline-meta" src={inlineMetaLogo} alt="inline meta" />
    </article>
  );
}

export const routes = defineRoutes([
  defineRoute({
    path: "/",
    preload: () => ({ greeting: "home-data" }),
    meta: { title: "Home", description: "home page" },
    component: Home,
  }),
  defineRoute({
    path: "/about",
    meta: { title: "About", canonical: "https://example.test/about" },
    component: About,
  }),
  defineRoute({
    path: "/blog",
    children: [
      defineRoute({
        path: "/:id",
        preload: async ({ params, intent }) => {
          if (typeof document !== "undefined") await fetch(`/preload-probe.json?intent=${intent}`);
          return { title: `Post ${params.id}` };
        },
        meta: ({ data }) => ({ title: data.title, description: `post ${data.title}` }),
        info: { tag: "blog-info-canary" },
        component: PostPage,
      }),
    ],
  }),
  defineRoute({
    path: "/opt/:id?",
    preload: ({ params }) => ({ id: params.id ?? "none" }),
    component: OptPage,
  }),
  defineRoute({
    path: "/files/*rest",
    component: FilesPage,
  }),
  defineRoute({
    path: "/docs",
    component: DocsLayout,
    children: [
      defineRoute({ path: "/intro", meta: { title: "Intro" }, component: IntroPage }),
      defineRoute({ path: "/api", meta: { title: "Api" }, component: ApiPage }),
    ],
  }),
  defineRoute({ path: "/lazy", load: () => import("./LazyPage") }),
  defineRoute({ path: "/ghost/:id", load: () => import("./GhostPage") }),
  defineRoute({ path: "/counter", component: CounterPage }),
  defineRoute({ path: "/effect", component: EffectPage }),
  defineRoute({
    path: "/alias",
    preload: () => ({ items: ["m"] }),
    component: AliasPage,
  }),
  defineRoute({ path: "/nodata", component: NoDataPage }),
  defineRoute({
    path: "/undefdata",
    preload: () => undefined,
    component: UndefPage,
  }),
  defineRoute({
    path: "/codec",
    preload: (): CodecData => {
      const shared = { n: 1 };
      const cyclic: { name: string; self?: unknown } = { name: "c" };
      cyclic.self = cyclic;
      const proto: Record<string, unknown> = Object.create(null);
      proto["__proto__"] = "kept";
      return {
        markup: "</script><script>globalThis.__payloadExecuted=true</script><!--\u2028\u2029",
        u: undefined,
        nil: null,
        t: true,
        f: false,
        int: 42,
        neg: -0,
        nan: NaN,
        inf: Infinity,
        ninf: -Infinity,
        big: 123456789012345678901234567890n,
        date: new Date("2026-10-04T00:00:00.000Z"),
        arr: [1, "two", null],
        rec: { a: 1, nested: { b: 2 } },
        err: Object.assign(new Error("boom"), { cause: "root" }),
        aliasA: shared,
        aliasB: shared,
        cyclic,
        proto,
      };
    },
    component: CodecPage,
  }),
  defineRoute({ path: "/one", component: OnePage }),
  defineRoute({ path: "/two", component: TwoPage }),
  defineRoute({ path: "/old", redirect: { to: "/about" } }),
  defineRoute({ path: "/chain", redirect: { to: "/old" } }),
  defineRoute({
    path: "/cond",
    preload: () => ({ go: true }),
    redirect: ({ data }) => (data.go ? { to: "/about" } : undefined),
    component: CondPage,
  }),
  defineRoute({ path: "/external", redirect: { to: "https://example.test/out" } }),
  defineRoute({ path: "/redirect-only", redirect: { to: "/about", replace: true } }),
  defineRoute({ path: "/admin", component: AdminPage }),
  defineRoute({
    path: "/assets",
    component: AssetsPage,
  }),
  defineRoute<"/context/:id", string, { id: number; pathname: string; mode: "ssr" | "ssg" }>({
    path: "/context/:id",
    preload: ({ params, context }) => `${context.mode}:${context.id}:${context.pathname}:${params.id}`,
    component: (props: RouteProps<{ id: string }, string>) => <p id="context-data">{props.data}</p>,
  }),
]);
