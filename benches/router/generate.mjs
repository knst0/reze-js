export const routers = ["reze", "solid-router", "react-router", "tanstack-router", "vue-router"];

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
    "main.jsx": `import { Router } from "@rezejs/router";
import { render } from "reze-js";
import { routes } from "virtual:reze-routes";

import { Shell } from "./Shell";

render(() => <Router routes={routes} root={Shell} />, document.getElementById("app"));
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

function solidRouter(sections) {
  const lazyRoute = (path, name, extra = "") => `{ path: "${path}", component: lazy(() => import("./pages/${name}"))${extra} }`;
  const routes = [
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
  return {
    "index.html": html("main.jsx"),
    "main.jsx": `import { Router } from "@solidjs/router";
import { lazy } from "solid-js";
import { render } from "solid-js/web";

import { getUser } from "./data";
import { Shell } from "./Shell";

const routes = [
  ${routes.join(",\n  ")},
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
    "data.js": `import { query } from "@solidjs/router";

export const getUser = query(async (id) => ({ name: "User " + id }), "user");
`,
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

function tanstackRouter(sections) {
  const link = (l) =>
    l.userId !== undefined
      ? `<Link to="/users/$id" params={{ id: "${l.userId}" }}>${l.label}</Link>`
      : `<Link to="${l.to}"${l.isRoot ? " activeOptions={{ exact: true }}" : ""}>${l.label}</Link>`;
  return {
    "index.html": html("main.jsx"),
    "main.jsx": `import { createRootRoute, createRoute, createRouter, lazyRouteComponent, RouterProvider } from "@tanstack/react-router";
import { createRoot } from "react-dom/client";

import { Shell } from "./Shell";

const page = (parent, path, load, options) =>
  createRoute({ getParentRoute: () => parent, path, component: lazyRouteComponent(load), ...options });

const rootRoute = createRootRoute({ component: Shell });
const usersRoute = page(rootRoute, "/users", () => import("./pages/Users"));

const routeTree = rootRoute.addChildren([
  page(rootRoute, "/", () => import("./pages/Home")),
  page(rootRoute, "/about", () => import("./pages/About")),
  usersRoute.addChildren([
    page(usersRoute, "/", () => import("./pages/UsersIndex")),
    page(usersRoute, "$id", () => import("./pages/User"), { loader: ({ params }) => ({ name: "User " + params.id }) }),
  ]),
${sectionIds(sections)
  .map((k) => `  page(rootRoute, "/section-${k}", () => import("./pages/Section${k}")),`)
  .join("\n")}
  page(rootRoute, "$", () => import("./pages/NotFound")),
]);

const router = createRouter({ routeTree });

${reactRender("<RouterProvider router={router} />")}
`,
    "Shell.jsx": `import { Link, Outlet } from "@tanstack/react-router";

export function Shell() {
  return (
    <>
      <nav>
${navLinks(sections)
  .map((l) => `        ${link(l)}`)
  .join("\n")}
      </nav>
      <main>
        <Outlet />
      </main>
    </>
  );
}
`,
    ...jsxPages(sections),
    "pages/Users.jsx": `import { Outlet } from "@tanstack/react-router";

export default function Users() {
  return (
    <section>
      <h1>Users</h1>
      <Outlet />
    </section>
  );
}
`,
    "pages/User.jsx": `import { getRouteApi, Link } from "@tanstack/react-router";

const route = getRouteApi("/users/$id");

export default function User() {
  const { id } = route.useParams();
  const data = route.useLoaderData();
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
const rezeBaseline = jsxBaseline(`import { render } from "reze-js";`, (el) => `render(() => ${el}, document.getElementById("app"));`);
const reactBaseline = jsxBaseline(`import { createRoot } from "react-dom/client";`, reactRender);

const generators = {
  reze: { router: reze, baseline: rezeBaseline },
  "solid-router": { router: solidRouter, baseline: solidBaseline },
  "react-router": { router: reactRouter, baseline: reactBaseline },
  "tanstack-router": { router: tanstackRouter, baseline: reactBaseline },
  "vue-router": { router: vueRouter, baseline: vueBaseline },
};

/** Source files of one app variant, keyed by path relative to `src/<variant>`. */
export function generate(router, variant, sections) {
  return generators[router][variant](sections);
}
