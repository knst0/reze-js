# Routing

Status: draft. Design of routes, segments, and route errors. It continues
[Server rendering](ssr.md) and [Interactivity](interactivity.md). Rules use MUST, NEVER, and
SHOULD in the RFC 2119 sense.

## Map files to URLs

Each file under `src/routes` is a route module. Folders add URL segments, file names add params:

| File                            | Pattern        |
| ------------------------------- | -------------- |
| `src/routes/index.tsx`          | `/`            |
| `src/routes/about.tsx`          | `/about`       |
| `src/routes/blog/index.tsx`     | `/blog`        |
| `src/routes/blog/[id].tsx`      | `/blog/:id`    |
| `src/routes/blog/[[page]].tsx`  | `/blog/:page?` |
| `src/routes/docs/[...slug].tsx` | `/docs/*slug`  |
| `src/routes/(auth)/login.tsx`   | `/login`       |

- `index` serves the URL of its folder.
- `[name]` is a required param, `[[name]]` an optional one, `[...name]` a catch-all that MUST be
  the last segment.
- A `(group)` folder adds no URL segment.
- A file named like its folder is that folder's layout: `blog.tsx` wraps every route in `blog/`.
- Files and folders starting with `_` or `.` are skipped, and so are `*.d.ts`, `*.test.*`, and
  `*.spec.*`.

When several routes match, segments compare left to right: a static segment beats a param, a
param beats a catch-all.

### Canonical URLs

Static segments match case-insensitively, and a trailing slash matches. The server answers such a
URL with `308` to its canonical form, so every page has one URL:

```
GET /Blog/7/  →  308 Location: /blog/7
```

## Segments

A **segment** is one layout or page together with its `load`. A URL renders a chain of segments,
from the root layout to the page.

The **key** of a segment is its module and the values of the inputs its loader reads. The compiler
knows which inputs a loader reads: its own params, `search`, or `request`.

```
/blog/3 → /blog/7
  root layout   key: root           → same, keeps its DOM
  blog layout   key: blog           → same, keeps its DOM
  [id] page     key: [id], id=7     → changed, loads and renders
```

On navigation, only segments whose key changed run their loaders and send HTML. The client
computes the change from the route table and asks for it with `_s`, see [Warm the next page](ssr.md#warm-the-next-page). A
loader that reads cookies through `request` does not rerun on navigation: only a mutation changes
cookies, and a mutation revalidates, see [Revalidate after a mutation](ssr.md#revalidate-after-a-mutation).

A segment without a loader has a key of its module alone.

## Layout loaders

A layout exports `load` like a page and reads its data through `props.data`. It types its props by
the URL prefix it wraps, `LayoutPropsFor<"/blog">`, the same way a page uses `RoutePropsFor`:

```tsx file=src/routes/blog.tsx
export const load = loader(async ({ context }) => ({ categories: await context.db.categories() }));

export default function BlogLayout(props: LayoutPropsFor<"/blog">) {
  return (
    <section>
      <nav>
        <For each={props.data.categories}>{(category) => <a href={category.href}>{category.name}</a>}</For>
      </nav>
      {props.children}
    </section>
  );
}
```

All loaders of a URL start at once. A layout loader NEVER waits for its parent, and a page loader
NEVER waits for its layouts. A loader that needs data of another segment loads it itself; a
request-scoped cache in `context` removes the duplicate query.

## Search params

Annotate `search` like params. The compiler coerces each value with the rules in
[Coerce form fields](ssr.md#coerce-form-fields):

```tsx
import type { U32 } from "reze-js/types";

export const load = loader(async ({ search }: { search: { page?: U32; q?: string } }) =>
  db.posts({ page: search.page ?? 1, query: search.q }),
);
```

- An optional param that fails coercion becomes `undefined`.
- A required param that is missing or fails coercion answers `400`.
- Params the annotation does not declare are ignored.

The path builder takes the same type:

```tsx
<a href={paths.blog.index({ page: 2 })}>Next</a>
```

A segment whose loader reads `search` includes the declared search params in its key. A change of
an undeclared param reruns nothing.

## Guards

Loaders of a URL run in parallel, so a `redirect` in a layout loader does not protect the page
loader: that loader already runs. A layout that protects its subtree exports `guard`:

```tsx file=src/routes/(app).tsx
import { guardRoute, redirect } from "reze-js/server";

export const guard = guardRoute(({ context }) => {
  if (!context.user) redirect(paths.login());
});
```

Guards run before every loader and mutation of their subtree, one after another from the root. A
guard decides from `context` and the URL and SHOULD NOT query anything itself: the context
already holds the session. A guard that throws stops the request with the matching status, see
[Redirects and route errors](#redirects-and-route-errors).

## Redirects and route errors

`redirect`, `notFound`, and `fail` come from `reze-js/server`. They work in loaders and mutations
and never return:

```tsx
export const load = loader(async ({ params, context }) => {
  if (!context.user) redirect(paths.login());
  const post = await context.db.post(params.id);
  if (!post) notFound();
  if (post.authorId !== context.user.id) fail(403, "Not your post");
  return post;
});
```

| Call                     | Status                                         |
| ------------------------ | ---------------------------------------------- |
| `redirect(path)`         | `307` from a loader, `303` from a mutation     |
| `redirect(path, status)` | The given `301`, `302`, `303`, `307`, or `308` |
| `notFound()`             | `404`                                          |
| `fail(status, message?)` | The given `4xx` status                         |
| Any other thrown value   | `500`                                          |

The path is typed through the path builders. During a client navigation, a redirect becomes a
navigation to its target.

Every loader of a URL finishes before the first byte, so the status is always known before the
response starts. Deferred fields cannot change the status: their errors render in their
`<Loading>` boundary.

### Catch route errors with `<Errored>`

A route error renders in the nearest `<Errored>` above the segment that threw. A layout places the
boundary where it wants the error to appear:

```tsx file=src/routes/blog.tsx
export default function BlogLayout(props: LayoutPropsFor<"/blog">) {
  return (
    <section>
      <BlogSidebar />
      <Errored fallback={(error) => <BlogError error={error} />}>{props.children}</Errored>
    </section>
  );
}

function BlogError(props: { error: RouteError }) {
  return (
    <Switch fallback={<p>Something broke.</p>}>
      <Match when={props.error.status === 404}>
        <p>No such post.</p>
      </Match>
    </Switch>
  );
}
```

Here the sidebar stays, and only the content becomes the error.

- The fallback receives a `RouteError`: `{ status, message }`. For a `500`, `message` is generic in
  production, so internal details never reach the page.
- A layout cannot catch the error of its own loader: the boundary of its parent catches it.
- A URL that matches no route is a `404`, caught by the root layout's boundary.
- Without any boundary, the server answers a minimal built-in page with the status.
- The fallback renders on the server and ships no client code unless it is interactive itself.

The same `<Errored>` catches handler errors in the browser, see [Async boundaries](api.md#async-boundaries).

## Client-rendered routes

A route renders in the browser when it exports `render`:

```tsx file=src/routes/editor.tsx
export const render = "client";
```

The server renders its layouts as usual and leaves the route's segment empty. The browser loads the
route's full component chunk and the loader data as JSON through `?_s=<depth>`, in the positional
encoding, with every field the component reads. Use it for pages that are interactive as a whole:
editors, canvases, dashboards.

### Client-only applications

`reze({ target: "spa" })` builds an application without a server: a static `index.html` and one
chunk per route. Routing uses the history or the hash. A loader or a mutation in such an
application is an error:

```tsx
export const load = loader(async () => …);
// SERVER_IN_SPA: `target: "spa"` has no server to run `load`.
// Fix: build with a server target, or fetch the data in an `effect`.
```

## Diagnostics

| Code            | Raised when                                     |
| --------------- | ----------------------------------------------- |
| `SERVER_IN_SPA` | A loader or mutation exists in a `"spa"` target |

## Implementation status

The current implementation differs from this design in these points:

- Route props come from `reze-js`: `RoutePropsFor`, `LayoutPropsFor`, and `RouteData` augment its
  `Register`. The generated file is `routes.gen.d.ts` next to the routes folder, `src/`.
- The path builder is `buildPaths("/blog/:page?", { page: 2 })`, keyed by the route pattern; it
  fills path params only. `redirect` takes the same `Href` type as `<a href>`.
- `LayoutPropsFor` keys layouts by URL prefix, so a group layout such as `(app).tsx` shares the key
  `/` with the root layout and cannot use it.
- A request with `_s` runs the loaders of that depth and below and renders those segments. When the
  render throws with no boundary inside them, the server runs the other loaders and answers the
  whole page. The client keys a segment by its route node, its params, and the search string when
  its loader reads `search` or its arguments cannot be read statically. A link to the current URL
  renders the last segment again.
- A boundary rolls back the HTML, head entries, and state rows its children produced before a
  server error. When its children add state rows, the server renders the boundary as
  `<!--e-->children<template data-rz-e>fallback</template>`, the fallback rendered with
  `RouteError(500, "Internal Server Error")`. When a handler inside throws or returns a promise
  that rejects, the client replaces the children with the template's content; without a boundary
  the error goes to `reportError`. Errors of effects are not caught.
- The compiled application exports `root`, the root layout chain, which renders the `404` of a URL
  that matches no route.
- When a guard throws a route error, the loaders above it run and the page renders the error at
  the guard's segment, so the layouts above keep their data; a redirect stops the request at once.
- A `POST` is checked for a cross-site origin before guards run, so a forged post answers `403`
  even when a guard would redirect.
- `target: "spa"` runs the compiled JS render in the browser: the navigation runtime answers each
  request with the app's handler instead of `fetch`, and morphs the result into the page as for a
  server response. The build writes `index.html`, the dispatcher, `links`, one module with the render of
  every route, and the client chunk of each route. Routing uses the history only, and the static
  host serves `index.html` for every path. `vite dev` renders on the server and does not run `src/client.ts`.
- A `render: "client"` page renders on the server as `<template data-rz-client>`, and its loader
  does not run for the document. The `links` module then navigates to the same URL, so the page
  arrives as the HTML of its segment through `?_s=<depth>`, not as JSON data that a client render
  turns into DOM. Without JavaScript the segment stays empty. The Rust server renders the page as
  any other.
