export const routers = [
  "reze",
  "solid-router",
  "solid-router-v2",
  "react-router",
  "tanstack-router",
  "octane-tanstack-router",
  "vue-router",
];

export const variants = ["router", "baseline"];

const html = (entry) => `<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <title>Router bench</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/${entry}"></script>
  </body>
</html>
`;

const sectionIds = (sections) => Array.from({ length: sections }, (_, k) => k);

function navLinks(sections) {
  return [
    { to: "/", label: "Home", isRoot: true },
    { to: "/about", label: "About" },
    { to: "/users", label: "Users" },
    ...[1, 2, 3].map((id) => ({ to: `/users/${id}`, label: `User ${id}`, userId: id })),
    ...sectionIds(sections).map((k) => ({ to: `/section-${k}`, label: `Section ${k}` })),
  ];
}

const staticPages = (sections) => [
  { name: "Home", file: "index", markup: `<h1 data-page="home">Home</h1>` },
  { name: "About", file: "about", markup: `<h1 data-page="about">About</h1>` },
  { name: "UsersIndex", file: "users/index", markup: `<p data-page="users">Pick a user</p>` },
  { name: "NotFound", file: "[...404]", markup: `<h1 data-page="404">Not found</h1>` },
  ...sectionIds(sections).map((k) => ({
    name: `Section${k}`,
    file: `section-${k}`,
    markup: `<h1 data-page="section-${k}">Section ${k}</h1>`,
  })),
];

const jsxPage = (page, exportName = `default function ${page.name}`) => `export ${exportName}() {\n  return ${page.markup};\n}\n`;

const jsxPages = (sections) => Object.fromEntries(staticPages(sections).map((page) => [`pages/${page.name}.jsx`, jsxPage(page)]));

const lazyPages = (sections) => staticPages(sections).filter((page) => page.name !== "Home");

const pagesGlobal = (sections, ext) =>
  `window.__pages = [\n${[...lazyPages(sections).map((p) => p.name), "Users", "User"]
    .map((name) => `  () => import("./pages/${name}${ext}"),`)
    .join("\n")}\n];\n`;

const jsxShell = (sections) => `export function Shell(props) {
  return (
    <>
      <nav>
${navLinks(sections)
  .map((link) => `        <a href="${link.to}">${link.label}</a>`)
  .join("\n")}
      </nav>
      <main>{props.children}</main>
    </>
  );
}
`;

const jsxUsers = `export default function Users(props) {
  return (
    <section>
      <h1>Users</h1>
      {props.children}
    </section>
  );
}
`;

function jsxBaseline(renderImport, renderCall) {
  return (sections) => ({
    "index.html": html("main.jsx"),
    "main.jsx": `${renderImport}
import Home from "./pages/Home";
import { Shell } from "./Shell";

${pagesGlobal(sections, "")}
${renderCall("<Shell><Home /></Shell>")}
`,
    "Shell.jsx": jsxShell(sections),
    ...jsxPages(sections),
    "pages/Users.jsx": jsxUsers,
    "pages/User.jsx": `export default function User(props) {
  return (
    <article data-page="user" data-id={props.id}>
      <h2>{props.name}</h2>
      <a data-next href={"/users/" + (Number(props.id) + 1)}>Next</a>
    </article>
  );
}
`,
  });
}

function reze(sections) {
  return {
    "index.html": html("main.jsx"),
    "main.jsx": `import { createRouter } from "@rezejs/router";
import { render } from "reze-js";
import { routes } from "virtual:reze-routes";

import { Shell } from "./Shell";

const Router = createRouter({ routes });

render(() => <Router root={Shell} />, document.getElementById("app"));
`,
    "Shell.jsx": jsxShell(sections),
    ...Object.fromEntries(staticPages(sections).map((page) => [`routes/${page.file}.jsx`, jsxPage(page)])),
    "routes/users.jsx": jsxUsers,
    "routes/users/[id].jsx": `export const route = { preload: ({ params }) => ({ name: "User " + params.id }) };

export default function User(props) {
  return (
    <article data-page="user" data-id={props.params.id}>
      <h2>{props.data.name}</h2>
      <a data-next href={"/users/" + (Number(props.params.id) + 1)}>Next</a>
    </article>
  );
}
`,
  };
}

function solidRoutes(sections) {
  const lazyRoute = (path, name, extra = "") => `{ path: "${path}", component: lazy(() => import("./pages/${name}"))${extra} }`;
  return [
    lazyRoute("/", "Home"),
    lazyRoute("/about", "About"),
    `{
    path: "/users",
    component: lazy(() => import("./pages/Users")),
    children: [
      ${lazyRoute("/", "UsersIndex")},
      ${lazyRoute("/:id", "User", ", preload: ({ params }) => getUser(params.id)")},
    ],
  }`,
    ...sectionIds(sections).map((k) => lazyRoute(`/section-${k}`, `Section${k}`)),
    lazyRoute("*404", "NotFound"),
  ];
}

const solidData = `import { query } from "@solidjs/router";

export const getUser = query(async (id) => ({ name: "User " + id }), "user");
`;

function solidRouter(sections) {
  return {
    "index.html": html("main.jsx"),
    "main.jsx": `import { Router } from "@solidjs/router";
import { lazy } from "solid-js";
import { render } from "solid-js/web";

import { getUser } from "./data";
import { Shell } from "./Shell";

const routes = [
  ${solidRoutes(sections).join(",\n  ")},
];

render(() => <Router root={Shell}>{routes}</Router>, document.getElementById("app"));
`,
    "Shell.jsx": `import { A } from "@solidjs/router";

export function Shell(props) {
  return (
    <>
      <nav>
${navLinks(sections)
  .map((link) => `        <A href="${link.to}"${link.isRoot ? " end" : ""}>${link.label}</A>`)
  .join("\n")}
      </nav>
      <main>{props.children}</main>
    </>
  );
}
`,
    ...jsxPages(sections),
    "pages/Users.jsx": jsxUsers,
    "data.js": solidData,
    "pages/User.jsx": `import { A, createAsync } from "@solidjs/router";

import { getUser } from "../data";

export default function User(props) {
  const user = createAsync(() => getUser(props.params.id));
  return (
    <article data-page="user" data-id={props.params.id}>
      <h2>{user()?.name}</h2>
      <A data-next href={"/users/" + (Number(props.params.id) + 1)}>
        Next
      </A>
    </article>
  );
}
`,
  };
}

function solidRouterV2(sections) {
  return {
    "index.html": html("main.jsx"),
    "main.jsx": `import { createRouter } from "@solidjs/router";
import { render } from "@solidjs/web";
import { lazy } from "solid-js";

import { getUser } from "./data";
import { Shell } from "./Shell";

const Router = createRouter({
  routes: [
    ${solidRoutes(sections).join(",\n    ")},
  ],
});

render(() => <Router>{(props) => <Shell>{props.children}</Shell>}</Router>, document.getElementById("app"));
`,
    "Shell.jsx": jsxShell(sections),
    ...jsxPages(sections),
    "pages/Users.jsx": jsxUsers,
    "data.js": solidData,
    "pages/User.jsx": `import { createMemo } from "solid-js";

import { getUser } from "../data";

export default function User(props) {
  const user = createMemo(() => getUser(props.params.id));
  return (
    <article data-page="user" data-id={props.params.id}>
      <h2>{user().name}</h2>
      <a data-next href={"/users/" + (Number(props.params.id) + 1)}>
        Next
      </a>
    </article>
  );
}
`,
  };
}

const reactRender = (element) => `createRoot(document.getElementById("app")).render(${element});`;

function reactRouter(sections) {
  const lazyRoute = (path, name) => `{ ${path === "" ? "index: true" : `path: "${path}"`}, lazy: () => import("./pages/${name}") }`;
  const routes = [
    lazyRoute("", "Home"),
    lazyRoute("about", "About"),
    `{
        path: "users",
        lazy: () => import("./pages/Users"),
        children: [${lazyRoute("", "UsersIndex")}, ${lazyRoute(":id", "User")}],
      }`,
    ...sectionIds(sections).map((k) => lazyRoute(`section-${k}`, `Section${k}`)),
    lazyRoute("*", "NotFound"),
  ];
  return {
    "index.html": html("main.jsx"),
    "main.jsx": `import { createRoot } from "react-dom/client";
import { createBrowserRouter } from "react-router";
import { RouterProvider } from "react-router/dom";

import { Shell } from "./Shell";

const router = createBrowserRouter([
  {
    path: "/",
    Component: Shell,
    children: [
      ${routes.join(",\n      ")},
    ],
  },
]);

${reactRender("<RouterProvider router={router} />")}
`,
    "Shell.jsx": `import { NavLink, Outlet } from "react-router";

export function Shell() {
  return (
    <>
      <nav>
${navLinks(sections)
  .map((link) => `        <NavLink to="${link.to}"${link.isRoot ? " end" : ""}>${link.label}</NavLink>`)
  .join("\n")}
      </nav>
      <main>
        <Outlet />
      </main>
    </>
  );
}
`,
    ...Object.fromEntries(staticPages(sections).map((page) => [`pages/${page.name}.jsx`, jsxPage(page, "function Component")])),
    "pages/Users.jsx": `import { Outlet } from "react-router";

export function Component() {
  return (
    <section>
      <h1>Users</h1>
      <Outlet />
    </section>
  );
}
`,
    "pages/User.jsx": `import { Link, useLoaderData, useParams } from "react-router";

export const loader = ({ params }) => ({ name: "User " + params.id });

export function Component() {
  const { id } = useParams();
  const data = useLoaderData();
  return (
    <article data-page="user" data-id={id}>
      <h2>{data.name}</h2>
      <Link data-next to={"/users/" + (Number(id) + 1)}>
        Next
      </Link>
    </article>
  );
}
`,
  };
}

const tanstackLink = (l) =>
  l.userId !== undefined
    ? `<Link to="/users/$id" params={{ id: "${l.userId}" }}>${l.label}</Link>`
    : `<Link to="${l.to}"${l.isRoot ? " activeOptions={{ exact: true }}" : ""}>${l.label}</Link>`;

const tanstackNav = (sections) =>
  navLinks(sections)
    .map((l) => `        ${tanstackLink(l)}`)
    .join("\n");

const tanstackRoutePath = (file) => (file === "index" ? "/" : file === "users/index" ? "/users/" : file === "[...404]" ? "/$" : `/${file}`);

const tanstackRouteFile = (file) => (file === "[...404]" ? "$" : file);

const fileRoute = (path, options, body, imports = "createFileRoute") => `import { ${imports} } from "@tanstack/react-router";

export const Route = createFileRoute("${path}")(${options});

${body}`;

function tanstackRouter(sections) {
  return {
    "index.html": html("main.jsx"),
    "main.jsx": `import { createRouter, RouterProvider } from "@tanstack/react-router";
import { createRoot } from "react-dom/client";

import { routeTree } from "./routeTree.gen";

const router = createRouter({ routeTree });

${reactRender("<RouterProvider router={router} />")}
`,
    "routes/__root.jsx": `import { createRootRoute, Link, Outlet } from "@tanstack/react-router";

export const Route = createRootRoute({ component: Shell });

function Shell() {
  return (
    <>
      <nav>
${tanstackNav(sections)}
      </nav>
      <main>
        <Outlet />
      </main>
    </>
  );
}
`,
    ...Object.fromEntries(
      staticPages(sections).map((page) => [
        `routes/${tanstackRouteFile(page.file)}.jsx`,
        fileRoute(
          tanstackRoutePath(page.file),
          `{ component: ${page.name} }`,
          jsxPage(page, `function ${page.name}`).replace(/^export /, ""),
        ),
      ]),
    ),
    "routes/users.jsx": fileRoute(
      "/users",
      "{ component: Users }",
      `function Users() {
  return (
    <section>
      <h1>Users</h1>
      <Outlet />
    </section>
  );
}
`,
      "createFileRoute, Outlet",
    ),
    "routes/users/$id.jsx": fileRoute(
      "/users/$id",
      `{
  loader: ({ params }) => ({ name: "User " + params.id }),
  component: User,
}`,
      `function User() {
  const { id } = Route.useParams();
  const data = Route.useLoaderData();
  return (
    <article data-page="user" data-id={id}>
      <h2>{data.name}</h2>
      <Link data-next to="/users/$id" params={{ id: String(Number(id) + 1) }}>
        Next
      </Link>
    </article>
  );
}
`,
      "createFileRoute, Link",
    ),
  };
}

const tsrxPage = (page) => `export default function ${page.name}() @{\n  ${page.markup}\n}\n`;

const tsrxPages = (sections) => Object.fromEntries(staticPages(sections).map((page) => [`pages/${page.name}.tsrx`, tsrxPage(page)]));

const octaneMain = `import { createRoot } from "octane";

import { App } from "./App.tsrx";

createRoot(document.getElementById("app")).render(App);
`;

function octaneTanstackRouter(sections) {
  return {
    "index.html": html("main.js"),
    "main.js": octaneMain,
    "App.tsrx": `import { createRootRoute, createRoute, createRouter, lazyRouteComponent, RouterProvider } from "@octanejs/tanstack-router";

import { Shell } from "./Shell.tsrx";

const page = (parent, path, load, options?) =>
  createRoute({ getParentRoute: () => parent, path, component: lazyRouteComponent(load), ...options });

const rootRoute = createRootRoute({ component: Shell });
const usersRoute = page(rootRoute, "/users", () => import("./pages/Users.tsrx"));

const routeTree = rootRoute.addChildren([
  page(rootRoute, "/", () => import("./pages/Home.tsrx")),
  page(rootRoute, "/about", () => import("./pages/About.tsrx")),
  usersRoute.addChildren([
    page(usersRoute, "/", () => import("./pages/UsersIndex.tsrx")),
    page(usersRoute, "$id", () => import("./pages/User.tsrx"), { loader: ({ params }) => ({ name: "User " + params.id }) }),
  ]),
${sectionIds(sections)
  .map((k) => `  page(rootRoute, "/section-${k}", () => import("./pages/Section${k}.tsrx")),`)
  .join("\n")}
  page(rootRoute, "$", () => import("./pages/NotFound.tsrx")),
]);

const router = createRouter({ routeTree });

export function App() @{
  <RouterProvider router={router} />
}
`,
    "Shell.tsrx": `import { Link, Outlet } from "@octanejs/tanstack-router";

export function Shell() @{
  <>
    <nav>
${tanstackNav(sections).replaceAll("        <", "      <")}
    </nav>
    <main>
      <Outlet />
    </main>
  </>
}
`,
    ...tsrxPages(sections),
    "pages/Users.tsrx": `import { Outlet } from "@octanejs/tanstack-router";

export default function Users() @{
  <section>
    <h1>Users</h1>
    <Outlet />
  </section>
}
`,
    "pages/User.tsrx": `import { getRouteApi, Link } from "@octanejs/tanstack-router";

const route = getRouteApi("/users/$id");

export default function User() @{
  const { id } = route.useParams();
  const data = route.useLoaderData();
  <article data-page="user" data-id={id}>
    <h2>{data.name as string}</h2>
    <Link data-next to="/users/$id" params={{ id: String(Number(id) + 1) }}>
      Next
    </Link>
  </article>
}
`,
  };
}

function octaneBaseline(sections) {
  return {
    "index.html": html("main.js"),
    "main.js": `import { createRoot } from "octane";

import { App } from "./App.tsrx";

${pagesGlobal(sections, ".tsrx")}
createRoot(document.getElementById("app")).render(App);
`,
    "App.tsrx": `import Home from "./pages/Home.tsrx";
import { Shell } from "./Shell.tsrx";

export function App() @{
  <Shell>
    <Home />
  </Shell>
}
`,
    "Shell.tsrx": `import type { OctaneNode } from "octane";

export function Shell({ children }: { children: OctaneNode }) @{
  <>
    <nav>
${navLinks(sections)
  .map((link) => `      <a href="${link.to}">${link.label}</a>`)
  .join("\n")}
    </nav>
    <main>{children}</main>
  </>
}
`,
    ...tsrxPages(sections),
    "pages/Users.tsrx": `import type { OctaneNode } from "octane";

export default function Users({ children }: { children: OctaneNode }) @{
  <section>
    <h1>Users</h1>
    {children}
  </section>
}
`,
    "pages/User.tsrx": `export default function User({ id, name }: { id: string; name: string }) @{
  <article data-page="user" data-id={id}>
    <h2>{name as string}</h2>
    <a data-next href={"/users/" + (Number(id) + 1)}>Next</a>
  </article>
}
`,
  };
}

const vuePage = (markup) => `<template>\n  ${markup}\n</template>\n`;

function vueNav(sections, tag) {
  return navLinks(sections)
    .map((link) =>
      tag === "a" ? `      <a href="${link.to}">${link.label}</a>` : `      <RouterLink to="${link.to}">${link.label}</RouterLink>`,
    )
    .join("\n");
}

function vueRouter(sections) {
  const lazyRoute = (path, name) => `{ path: "${path}", component: () => import("./pages/${name}.vue") }`;
  const routes = [
    lazyRoute("/", "Home"),
    lazyRoute("/about", "About"),
    `{
      path: "/users",
      component: () => import("./pages/Users.vue"),
      children: [${lazyRoute("", "UsersIndex")}, ${lazyRoute(":id", "User")}],
    }`,
    ...sectionIds(sections).map((k) => lazyRoute(`/section-${k}`, `Section${k}`)),
    lazyRoute("/:pathMatch(.*)*", "NotFound"),
  ];
  return {
    "index.html": html("main.js"),
    "main.js": `import { createApp } from "vue";
import { createRouter, createWebHistory } from "vue-router";

import Shell from "./Shell.vue";

const router = createRouter({
  history: createWebHistory(),
  routes: [
    ${routes.join(",\n    ")},
  ],
});

createApp(Shell).use(router).mount("#app");
`,
    "Shell.vue": `<template>
  <nav>
${vueNav(sections, "RouterLink")}
  </nav>
  <main>
    <RouterView />
  </main>
</template>
`,
    ...Object.fromEntries(staticPages(sections).map((p) => [`pages/${p.name}.vue`, vuePage(p.markup)])),
    "pages/Users.vue": vuePage(`<section>
    <h1>Users</h1>
    <RouterView />
  </section>`),
    "pages/User.vue": `<script setup>
import { computed } from "vue";
import { useRoute } from "vue-router";

const route = useRoute();
const id = computed(() => route.params.id);
const name = computed(() => "User " + id.value);
</script>

<template>
  <article data-page="user" :data-id="id">
    <h2>{{ name }}</h2>
    <RouterLink data-next :to="'/users/' + (Number(id) + 1)">Next</RouterLink>
  </article>
</template>
`,
  };
}

function vueBaseline(sections) {
  return {
    "index.html": html("main.js"),
    "main.js": `import { createApp } from "vue";

import Shell from "./Shell.vue";

${pagesGlobal(sections, ".vue")}
createApp(Shell).mount("#app");
`,
    "Shell.vue": `<script setup>
import Home from "./pages/Home.vue";
</script>

<template>
  <nav>
${vueNav(sections, "a")}
  </nav>
  <main>
    <Home />
  </main>
</template>
`,
    ...Object.fromEntries(staticPages(sections).map((p) => [`pages/${p.name}.vue`, vuePage(p.markup)])),
    "pages/Users.vue": vuePage(`<section>
    <h1>Users</h1>
    <slot />
  </section>`),
    "pages/User.vue": `<script setup>
const props = defineProps({ id: String, name: String });
</script>

<template>
  <article data-page="user" :data-id="props.id">
    <h2>{{ props.name }}</h2>
    <a data-next :href="'/users/' + (Number(props.id) + 1)">Next</a>
  </article>
</template>
`,
  };
}

const solidBaseline = jsxBaseline(`import { render } from "solid-js/web";`, (el) => `render(() => ${el}, document.getElementById("app"));`);
const solidV2Baseline = jsxBaseline(
  `import { render } from "@solidjs/web";`,
  (el) => `render(() => ${el}, document.getElementById("app"));`,
);
const rezeBaseline = jsxBaseline(`import { render } from "reze-js";`, (el) => `render(() => ${el}, document.getElementById("app"));`);
const reactBaseline = jsxBaseline(`import { createRoot } from "react-dom/client";`, reactRender);

const generators = {
  reze: { router: reze, baseline: rezeBaseline },
  "solid-router": { router: solidRouter, baseline: solidBaseline },
  "solid-router-v2": { router: solidRouterV2, baseline: solidV2Baseline },
  "react-router": { router: reactRouter, baseline: reactBaseline },
  "tanstack-router": { router: tanstackRouter, baseline: reactBaseline },
  "octane-tanstack-router": { router: octaneTanstackRouter, baseline: octaneBaseline },
  "vue-router": { router: vueRouter, baseline: vueBaseline },
};

/** Source files of one app variant, keyed by path relative to `src/<variant>`. */
export function generate(router, variant, sections) {
  return generators[router][variant](sections);
}
