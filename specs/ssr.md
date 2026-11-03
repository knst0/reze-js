# Server rendering

Status: draft. Design of server rendering for Reze, written from scratch. It records the
decisions from the design discussion of 2026-11-02. Rules use MUST, NEVER, and SHOULD in the
RFC 2119 sense.

## Goals

- Keep the runtime small and move work into the compiler. Every rule the compiler can enforce
  is a compiler rule, not a runtime API.
- Let the server be written in Rust or in Node.js with one route model, one type contract, and
  one wire format.
- Make route data type-safe without writing a schema. A schema is an opt-in for rules that
  types cannot express.
- Ship no JS for static nodes. The compiler decides what runs on the client; users never mark it.
- Refuse what the compiler cannot prove. A shape the compiler cannot lower is an error with a
  fix, never a silent fallback.

## Model

An application is a set of route modules. A route module exports a `load` loader and a
component. Mutations are `mutation` functions. From these sources the compiler produces:

```
route modules ──► reze compiler ─┬─► client chunks: handlers and the bindings they update
                                 ├─► server render: Rust source (build.rs) or JS
                                 ├─► server validators for `mutation` inputs
                                 └─► type IR and TS types for every loader and client state
```

The browser receives HTML for the whole page and no JS until the user starts an interaction;
[Interactivity](interactivity.md) describes how handlers load. Data crosses from server to
browser only as client state, encoded as positional JSON.

`action` names the reactive primitive. Server writes are named `mutation`: with `loader` they
form the pair read and write, and `action` never collides.

## Route data: `loader`

### Declare a loader

A route module exports `load`:

```tsx file=src/routes/blog/[id].tsx
import type { RoutePropsFor } from "@rezejs/router";
import { loader } from "reze-js/server";

export const load = loader(async ({ params, context, request }) => {
  const post = await context.db.post(params.id);
  return { title: post.title, body: post.body, tags: post.tags };
});

export default function Post(props: RoutePropsFor<"/blog/:id">) {
  return (
    <article>
      <h2>{props.data.title}</h2>
      <ul>
        <For each={props.data.tags}>{(tag) => <li>{tag}</li>}</For>
      </ul>
    </article>
  );
}
```

The loader receives:

| Field     | Type                   | Source                                                |
| --------- | ---------------------- | ----------------------------------------------------- |
| `params`  | Typed from the pattern | The matched URL                                       |
| `context` | Application-defined    | The value the server entry passes for this request    |
| `request` | `Request`              | The incoming request; absent when prerendering a page |

A loader runs only on the server. Its code NEVER reaches the client bundle.

### Type props through the route

A component types its props by pattern. The user NEVER writes `typeof load`.

The compiler generates route declarations that bind each pattern to its module's loader:

```ts
// generated: routes.gen.d.ts
declare module "@rezejs/router" {
  interface Register {
    routes: {
      "/blog/:id": {
        params: { id: string };
        data: Awaited<ReturnType<typeof import("./routes/blog/[id]").load>>;
      };
    };
  }
}
```

The TypeScript checker does the inference. The compiler only links the pattern to the module.

### Type params

Params from a pattern are strings. To get another type, annotate the loader argument:

```tsx
export const load = loader(async ({ params }: { params: { id: number } }) => db.post(params.id));
```

The compiler lowers the annotation with the rules in [Annotated input](#annotated-input) and
coerces the segment before the loader runs. A segment that fails coercion answers `404`: no route
matches `/blog/abc` when `id` is a number.

### Validate a loader result

A loader's result comes from the application's own server, so it is not validated by default.
To validate it, pass a Standard Schema v1 as the first argument:

```tsx
import * as v from "valibot";

const Post = v.object({ title: v.string(), tags: v.array(v.string()) });

export const load = loader(Post, async ({ params }) => cms.fetch(params.id));
```

Use this when the data comes from a source the application does not control, such as a CMS or a
third-party API. Any Standard Schema v1 implementation works: zod, valibot, arktype, or
`reze-js/schema`. The schema runs only on the server and adds no client bytes.

### Type loader data the client reads

Client state crosses the wire as positional JSON, which needs a type. Most loader data only fills HTML on
the server and needs no type beyond what TypeScript infers. When a client chunk reads a loader
field, the loader MUST annotate its result:

```tsx
interface Post {
  title: string;
  likes: number;
}

export const load = loader(async ({ params }): Promise<Post> => db.post(params.id));
// or
export const load = loader<Post>(async ({ params }) => db.post(params.id));
```

The second form matches `loader.external<Post>()`. The compiler lowers the annotation with the
rules in [Annotated input](#annotated-input) and trusts it; `tsc` checks that the loader returns
a `Post`.

Without the annotation, the compiler reports the read that needs it:

```tsx
export const load = loader(async ({ params }) => db.post(params.id));

export default function Post(props: RoutePropsFor<"/blog/:id">) {
  let likes = signal(props.data.likes);
  return <button onClick={() => likes++}>{likes}</button>;
}
// LOADER_RESULT_UNTYPED: the client reads `props.data.likes`, but the loader of `/blog/:id` has
// no result type.
// Fix: annotate the result, `loader<Post>(…)` or `async (): Promise<Post> => …`.
```

The compiler NEVER asks the TypeScript checker for the inferred type. An inferred type often
holds what the wire cannot carry, such as ORM classes with methods or decimal types, and an error
would then point at a type the user never wrote.

## Mutations: `mutation`

### Validate input

Every `mutation` is a public POST endpoint, so its input MUST be validated at runtime.
TypeScript types do not exist at runtime: the compiler generates the validator from the input
annotation, or the user passes a schema. Annotation is the default; a schema is for rules that
types cannot express.

Annotation describes the **shape** of the data:

```tsx
interface NewTodo {
  title: string;
  done?: boolean;
  priority: 1 | 2 | 3;
  tags: string[];
}

export const createTodo = mutation(async (input: NewTodo, { request }) => db.insert(input));
```

A schema adds **rules**:

```tsx
import { z } from "zod";

export const signup = mutation(z.object({ email: z.string().email(), password: z.string().min(12) }), async (input) =>
  accounts.create(input),
);
```

Use the annotation for shape-only input such as ids, flags, and enums. Use a schema for formats,
lengths, ranges, and patterns. The choice does not affect client bytes: both validators exist
only in the server bundle. The client bundle receives the endpoint and a call stub.

### Generated validator

For `NewTodo` above, the server bundle receives a validator specialized to the type:

```js
function checkNewTodo(x) {
  if (typeof x !== "object" || x === null) return fail("input");
  if (typeof x.title !== "string") return fail("input.title");
  if (x.done !== undefined && typeof x.done !== "boolean") return fail("input.done");
  if (x.priority !== 1 && x.priority !== 2 && x.priority !== 3) return fail("input.priority");
  if (!Array.isArray(x.tags)) return fail("input.tags");
  for (const tag of x.tags) if (typeof tag !== "string") return fail("input.tags[]");
  return x;
}
```

No generic validation runtime ships. Each check is straight-line code for one type.

### Annotated input

The compiler resolves an annotation syntactically. It MUST accept:

- Primitives: `string`, `number`, `boolean`, `bigint`, `null`.
- Literals and unions of literals: `"draft" | "published"`, `1 | 2 | 3`.
- Object literal types, `interface`, and `type` aliases declared in the same file or imported
  with a relative path from the project.
- Optional properties (`done?: boolean`), `T | undefined`, and `T | null`.
- `T[]`, `Array<T>`, readonly arrays, and tuples.
- `Record<string, T>`, `Map<string, T>`, and `Set<T>`.
- `Date`, `Uint8Array`, and `File`.
- Discriminated unions with a literal tag property.
- The integer aliases from `reze-js/types` (see [Encode values](#encode-values)).

It MUST refuse anything else, with a diagnostic that names the fix:

```tsx
mutation(async (input: Pick<Todo, "title">) => …);
// MUTATION_INPUT_UNRESOLVED: mapped and utility types are not lowered.
// Fix: write the object type out, or pass a schema.

mutation(async (input: unknown) => …);
// MUTATION_INPUT_UNTYPED: a public endpoint needs a contract.
// Fix: annotate the input, or pass a schema.

mutation(async (input) => …);
// MUTATION_INPUT_UNTYPED: same as above.

mutation(async (input: SignupForm) => …); // SignupForm from a package without a `reze` export
// MUTATION_INPUT_UNRESOLVED: types from packages without source are not resolved.
// Fix: declare the type in the project, or pass a schema.

mutation(async (input: { when: Temporal.Instant }) => …);
// MUTATION_INPUT_UNRESOLVED: `Temporal.Instant` has no lowering.
// Fix: use `Date`, or pass a schema.
```

### Coerce form fields

A plain `<form>` sends every field as a string. The compiler knows the target type, so it
coerces form fields before validation:

| Annotated type   | Form value          | Result                                  |
| ---------------- | ------------------- | --------------------------------------- |
| `string`         | any                 | The string                              |
| `number`         | `"42"`, `"-1.5"`    | `42`, `-1.5`                            |
| `number`         | `""`, `"abc"`       | Rejected                                |
| `boolean`        | `"on"`, `"true"`    | `true`                                  |
| `boolean`        | missing, `"false"`  | `false`                                 |
| `1 \| 2 \| 3`    | `"2"`               | `2`; any other value is rejected        |
| `string[]`       | repeated field      | Every value, in order                   |
| `T \| undefined` | missing or `""`     | `undefined`                             |
| `Date`           | ISO 8601 string     | `Date`; an unparsable value is rejected |
| `File`           | multipart file part | `File`                                  |

A JSON call through `.call(input)` is NEVER coerced: it carries typed values already.

### Bind a form to a mutation

Pass the mutation to `action`. The compiler lowers it to the method, the endpoint, the return
path, and the progressive enhancement:

```tsx
export default function Todos(props: RoutePropsFor<"/todos">) {
  return (
    <form action={createTodo}>
      <input name="title" />
      <input name="done" type="checkbox" />
      <button type="submit">Add</button>
    </form>
  );
}
```

The user NEVER writes `method`, a return path, or the endpoint URL. The compiler checks the
form's named fields against the input type and reports a field the type does not declare.

### Post to the page URL

A mutation posts to the URL of the page that renders it, with the mutation id in `_m`:

```html
<form method="post" action="/todos?page=2&_m=k3x9f2">…</form>
```

The server then knows which page to answer with: the `303` back to it, or the page rendered again
with `422`. It NEVER relies on `Referer`, which `Referrer-Policy` can strip.

`.call(input)` posts to the same URL with a JSON body. Its response carries the
result together with the HTML of the revalidated segments, so a call and a form revalidate the
same way. The input is named JSON, because a schema validator reads names; the result uses the
positional encoding and needs a result annotation when client code reads it, like a loader.

The id is base-36 of a hash of the module path and the export name. It stays the same across
builds, so a form on a page opened before a deploy reaches the same mutation. The compiler reports
two mutations whose ids collide. A renamed file or export changes the id, and an old form then
gets `404`; a changed input type gets `422`.

Mutations accept only `POST`. Loaders only read, so a `GET` never changes state.

### Reject cross-site posts

The server rejects a cross-site mutation with `403`, without tokens:

1. `Sec-Fetch-Site: cross-site` is rejected; `same-origin`, `same-site`, and `none` pass.
2. Without `Sec-Fetch-Site`, `Origin` MUST match `Host`.
3. Without both headers, the request does not come from a browser, so it cannot carry a forged
   cross-site request, and it passes.

`trustedOrigins` lists other origins that may post. Tokens are not used: they cost bytes on every
page and need session state.

The body is limited to 1 MB for JSON and form fields by default, with a separate limit for files.
Both are configurable. Authorization of a particular mutation is the mutation's own code, through
`context`; rate limiting belongs to the host.

### Revalidate after a mutation

After a mutation, the server reruns every loader of the page that called it and renders the route
segments whose loaders reran. Pass `revalidate` to narrow the set to the routes the mutation
changes:

```tsx
export const toggleTodo = mutation(async (input: { id: number }) => db.toggle(input.id), {
  revalidate: ["/todos"],
});
```

The paths are checked against the route table, like `RoutePropsFor`. A segment outside the list
keeps its HTML. [Server](interactivity.md#server) specifies the response and how the client
applies it.

### Report invalid input

Input that fails validation answers `422`. A `.call(input)` receives the issues as JSON, in the
Standard Schema format, whichever validator produced them:

```json
{ "issues": [{ "path": ["title"], "message": "Expected a string" }] }
```

`.call` throws a `MutationInputError` that carries `issues`. `<Errored>` catches it like any
other handler error.

A form post that fails validation does not redirect. The server renders the whole page again with
`422`, and the page reads the submission with `useSubmission`:

```tsx
export default function Todos() {
  const submission = useSubmission(createTodo);
  return (
    <form action={createTodo}>
      <input name="title" value={submission.input?.title ?? ""} />
      <Show when={submission.issues?.title}>{(issue) => <p role="alert">{issue.message}</p>}</Show>
      <button type="submit">Add</button>
    </form>
  );
}
```

`useSubmission(mutation)` returns `{ input, issues, result }` for the current request: the raw
form values, the issues keyed by path, and the mutation result. Each field is `undefined` when
the request did not submit that mutation. The server evaluates it, so a hole that reads it is a
server hole and ships no client code. With the dispatcher, the `422` page arrives through
`fetch` and the morph applies it, so the errors appear without a reload and without client code.

## Request context

### Build the context

The server entry builds `context` once per request. Declare its type once:

```ts file=src/server.ts
import { createHandler } from "reze-js/server";

export default createHandler({
  context: async (request) => ({ db, user: await sessionUser(request) }),
});

declare module "reze-js/server" {
  interface Register {
    context: AppContext;
  }
}
```

In Rust, `App<C>` is generic over the context, and the adapter builds `C` from the request parts.

Reze has no middleware chain. Compose middleware in the host: tower layers in Rust, any
fetch-handler framework in JS. Sessions and signed cookies belong to libraries.

### Set headers and cookies

Loaders and mutations receive `response`:

```tsx
export const load = loader(async ({ response }) => {
  response.headers.set("Cache-Control", "max-age=60");
  response.cookies.set("seen", "1");
  return …;
});
```

`response.cookies.set` defaults to `HttpOnly`, `Secure`, `SameSite=Lax`, and `Path=/`.

Headers exist only before the first byte. Only loaders that are not deferred and mutations set
them; a write from a deferred field throws.

A page's `Cache-Control` is the strictest of its segments: the smallest `max-age` wins, and
`private` wins over `public`.

## Client code

The server renders a page that is visually complete and runs no JS. Handlers load when the user
starts an interaction, together with the bindings they update. Static nodes NEVER receive client
code. [Interactivity](interactivity.md) specifies the model, the HTML metadata, and the loading
strategy.

A client navigation fetches the next page as HTML of the route segments that change, together
with their state block, and applies it with the same morph as a server-tier response. Segments
shared with the current page, such as layouts, keep their DOM.

### Focus and scroll after navigation

After a navigation, focus moves to the first `<h1>` of the first changed segment, with a temporary
`tabindex="-1"`, or to `<main>` when the segment has no `<h1>`. A screen reader reads the new
heading, so no live region is needed. `document.title` follows the `<head>` of the new segments.

A navigation that changes only search params or the hash, such as a filter, keeps focus where it
is.

| Navigation                         | Scroll                                |
| ---------------------------------- | ------------------------------------- |
| To a new page                      | Top, or the element the hash names    |
| Back and forward                   | The position saved in `history.state` |
| Scroll containers of kept segments | Unchanged: the morph keeps them       |

The client sets `history.scrollRestoration = "manual"` and saves the position before each
navigation.

### Hook into the swap

Before the client applies a morph, it dispatches a cancelable `rz:swap` event on `window`.
`event.detail.apply()` applies the morph, and `event.detail.kind` is `"navigation"` or
`"mutation"`. Without a listener that calls `preventDefault()`, the client applies the morph at
once.

Reze has no view transition option. A page that wants transitions for client navigations wraps
the swap itself:

```ts
addEventListener("rz:swap", (event) => {
  if (event.detail.kind !== "navigation") return;
  event.preventDefault();
  document.startViewTransition(() => event.detail.apply());
});
```

Full document navigations (without JS, after a `409`, through `location.assign`) need only CSS:

```css
@view-transition {
  navigation: auto;
}
```

Transitions are never on by default: without the page's own CSS, the browser cross-fades the
whole page and ignores input while it runs. The same event serves custom animations, analytics,
and navigation timing.

### Warm the next page

A link uses the same intent events as a handler site: `pointerenter` from a mouse or a pen,
`pointerdown`, and `focusin`. On intent, the client fetches the next page before the click:

```http
GET /blog/7?_s=2&_b=k2f9
Purpose: prefetch
```

The client knows the route table and which inputs each segment's loader reads, so it computes the
first segment that changes, see [Segments](routing.md#segments). `_s` is its depth: the server
answers that segment and the ones below it. The response lists the client chunks of the next
route, and the client preloads them with `modulepreload`. Route ids NEVER appear in the HTML.

A response depends only on its URL, so a CDN caches it without `Vary`. A URL has as many segment
variants as the route has levels.

The client keeps a prefetched response for 30 seconds or until the next mutation, whichever comes
first. Prefetching is safe because loaders only read. It is off under `Save-Data`.

### One build on both sides

The page and the server MUST come from the same build. Every `fetch` of the client (navigation,
prefetch, server-tier posts) carries the build id in `_b`. The id is part of the URL, so a CDN
never serves a response cached for another build. When it differs from the server's build, the
server answers `409` and the client performs a full navigation with `location.assign`. A deploy
therefore never mixes state layouts or chunk manifests from two builds.

## Streaming

### Status before the first byte

The server awaits every loader field that is not deferred before it sends the status, so `404`
and redirects stay real responses. While loaders run, the server sends `103 Early Hints` with the
route's stylesheets, fonts, and `modulepreload` links, where the server stack supports it. The URL
alone determines the route, so the hints need no loader.

### Defer slow fields

A loader marks a slow field with `defer`, and the page wraps its reads in `<Loading>`:

```tsx
export const load = loader(async ({ params }) => ({
  post: await db.post(params.id),
  comments: defer(db.comments(params.id)),
}));
```

The server streams the document in order. At a `<Loading>` boundary it writes the fallback and
continues. Each deferred part is appended to the end of the document when its data resolves, in
resolution order, together with its part of the state block:

```html
<section data-rz-l="0"><p>Loading comments…</p></section>
…
<template data-rz-s="0"
  ><ul>
    <li>First!</li>
  </ul></template
>
```

A separate `stream` module moves each template into its boundary. The server adds it only to
routes with deferred fields. It processes the templates that arrived before it loaded, then
watches for more with a `MutationObserver`. The document contains no inline script, so a strict
Content Security Policy needs no nonce.

Without JS, a deferred part never leaves its template. Defer only secondary content: comments,
recommendations, counters. Content the page exists for MUST NOT be deferred. Prerendering awaits
every deferred field and streams nothing.

### Rust

`reze_server` returns the body as a stream of `Bytes`. The generated render function writes into
a writer, and each deferred hole becomes a future that the stream polls after the main document.

## Wire format: positional JSON

### One contract for both languages

Client state crosses from server to browser as positional JSON, for TS and Rust loaders alike. A
browser never knows which language produced its data.

Positional JSON carries no field names and no type tags. A struct is an array of its fields in
declaration order. The compiler knows every type at build time, so it reads values by index and
needs no schema at run time:

```
Rust types ──#[derive(reze::Data)]──► IR ─┐
                                          ├─► reze compiler ─┬─► TS types for props.data
TS annotations ──────lowering───────► IR ─┘                  ├─► reads by index in client chunks
                                                             └─► Rust render code
```

The IR is the compiler's own type description. It also drives the generated validators, form
coercion, and the TS types of Rust data.

### What crosses the wire

The server renders from native values and never parses its own state. Positional JSON carries only
what the browser needs: client state. Loader results that no handler or binding reads never leave
the server.

### Read only what bindings read

The compiler encodes, per client chunk, only the fields that the chunk's handlers and bindings
read. A field only the server uses NEVER reaches the client.

```tsx
// Post is { title, body, tags, authorEmail }
function LikeButton(props: { post: Post }) {
  let liked = signal(false);
  return (
    <button
      onClick={() => {
        liked = !liked;
        if (liked) share(props.post.title);
      }}
    >
      Like
    </button>
  );
}
```

Only `title` is encoded for `LikeButton`. `body`, `tags`, and `authorEmail` stay on the server.

No decoder ships. `JSON.parse` reads the state block, and the compiler rewrites every read into an
index access:

```js
// props.post.title
s[3][0];
```

### Encode values

| TypeScript                                       | Positional JSON                             | Read as                  |
| ------------------------------------------------ | ------------------------------------------- | ------------------------ |
| `string`, `boolean`                              | The value                                   | The value                |
| `number`                                         | The value; `NaN` and infinities are refused | The value                |
| `U8`, `U16`, `U32`, `I8`, `I16`, `I32`           | The value, range-checked by the server      | `number`                 |
| `U64`, `I64`                                     | Decimal string                              | `BigInt(v)`              |
| `Date`                                           | Milliseconds since the epoch                | `new Date(v)`            |
| `Uint8Array`                                     | base64 string                               | Decoded at the read site |
| `T[]`, `[A, B]`                                  | Array                                       | Array                    |
| `Set<T>`                                         | Array                                       | `new Set(v)`             |
| `Map<K, V>`, `Record<string, V>`                 | Array of `[key, value]` pairs               | `new Map(v)`, an object  |
| `T \| null`, optional property, `T \| undefined` | `null` when absent                          | `null` or `undefined`    |
| `T \| null` that is also optional                | `[]` when undefined, `[v]` otherwise        | The value                |
| object type, `interface`                         | Array of fields in declaration order        | Index access             |
| union of literals                                | Index of the member in declaration order    | A table lookup           |
| discriminated union                              | `[variant index, ...fields]`                | Index access             |

The integer aliases come from `reze-js/types`. Rust types map one-to-one through
`#[derive(reze::Data)]`. Generated TS types follow the table in reverse.

Trailing absent optional fields are dropped from a struct's array. A conversion runs at the read
site and only for the fields a chunk reads.

Reordering fields or union members is safe: the page and the server always come from one build,
see [One build on both sides](#one-build-on-both-sides).

The state block is inline in the HTML, and `fetch` responses carry their part of it next to their
HTML, both as positional JSON. No binary encoding exists: inline it would need base64, which adds
33%, and `fetch` responses are mostly HTML.

### Rust loaders

A route whose loader lives in Rust declares only the data type in its module:

```tsx file=src/routes/blog/[id].tsx
import type { RoutePropsFor } from "@rezejs/router";
import { loader } from "reze-js/server";
import type { Post } from "virtual:reze-types/app_types";

export const load = loader.external<Post>();

export default function PostPage(props: RoutePropsFor<"/blog/:id">) {
  return <h2>{props.data.title}</h2>;
}
```

`virtual:reze-types/<crate>` exposes TS types generated from that crate's IR. The Rust side
implements the loader:

```rust
// server/src/routes.rs
use app_types::Post;
use reze_server::{Ctx, Result};

use crate::generated::routes::blog_id::Params;

pub async fn blog_id(params: Params, ctx: &Ctx) -> Result<Post> {
    ctx.db().post(params.id).await
}
```

```rust
// server/src/main.rs
let app = generated::routes::App::new().blog_id(routes::blog_id);
```

The generated `blog_id` setter takes `fn(Params, &Ctx) -> impl Future<Output = Result<Post>>`. A
loader that returns another type is a rustc error, so the template, the TS types, and the Rust
loader cannot disagree.

## Render target: Rust codegen

### Emit Rust source

The compiler emits the server render of each route as Rust source, compiled by `rustc` together
with the application server. A template that reads a field the Rust type does not have fails to
compile. No template interpreter runs at request time.

### Build pipeline

Keep loader data types in their own crate, so their IR exists before the server crate builds:

```
app_types/        #[derive(reze::Data)] structs and enums
server/
  build.rs        reze_build::compile(…)
  src/main.rs     include!(concat!(env!("OUT_DIR"), "/reze_routes.rs"))
```

1. `cargo xtask reze-ir app_types` writes the IR of `app_types` as JSON.
2. The reze compiler reads the routes and the IR, then writes TS types and client bundles.
3. `server/build.rs` calls `reze_build::compile`, which writes `reze_routes.rs`: params structs,
   one render function per route level, and the `App` builder.

The separate types crate breaks the cycle between "IR comes from Rust types" and "Rust render
code comes from templates".

### Generated code

For the `Post` component above, the compiler emits:

```rust
pub fn render_blog_id(out: &mut Html, params: &Params, data: &app_types::Post) {
    out.raw("<article><h2>");
    out.text(&data.title);
    out.raw("</h2><ul>");
    for tag in &data.tags {
        out.raw("<li>");
        out.text(tag);
        out.raw("</li>");
    }
    out.raw("</ul></article>");
}
```

The static parts are string constants decided at build time. Only holes cost work per request.

### Hole expressions

Output MUST be identical across the client, JS server, and Rust server targets. A hole
in the server path MUST be a projection of `params`, `data`, `context`, and loop variables. The
compiler lowers:

| Expression                                 | Example                         |
| ------------------------------------------ | ------------------------------- |
| Field access, optional chaining            | `data.author?.name`             |
| Literals, template literals                | `` `${data.count} items` ``     |
| `??`, `&&`, `\|\|`, `!`, ternary on values | `data.subtitle ?? data.title`   |
| Comparisons, `===`, `!==`                  | `data.count > 0`                |
| Arithmetic on numbers                      | `data.page + 1`                 |
| `.length` on strings and arrays            | `data.tags.length`              |
| `<Show>`, `<For>`, `<Switch>`/`<Match>`    | as in the client target         |
| Calls to project components                | `<Avatar user={data.author} />` |

Values render exactly as JS renders them:

- Numbers format through a JS-compatible algorithm (`Number.prototype.toString`), so `1e21`,
  `-0`, and `0.1 + 0.2` print the same in Rust and in the browser.
- HTML escaping is one specification with one conformance suite for both languages.
- String length counts UTF-16 code units, as `.length` does in JS.

### Non-lowerable holes are errors

A hole the compiler cannot lower is a compile error. The compiler NEVER moves it to the client or
to a JS fallback on its own.

```tsx
<time>{new Intl.DateTimeFormat("de").format(data.at)}</time>
// SERVER_HOLE_UNSUPPORTED: `Intl.DateTimeFormat` has no Rust lowering.
// Fix: format the value in the loader, or make the element interactive on the client.

<p>{slugify(data.title)}</p> // slugify from an npm package
// SERVER_HOLE_UNSUPPORTED: calls to unanalyzed functions are not lowered.
// Fix: compute the value in the loader.

<p>{data.tags.sort().join(", ")}</p>
// SERVER_HOLE_UNSUPPORTED: `sort` mutates and has no lowering.
// Fix: sort in the loader.
```

The fix is always one of two: compute the value in the loader, which keeps the template a
projection, or compute it on the client deliberately.

### Node.js

A Node.js server renders through the JS server target, built from the same templates under the
same hole rules. Moving a route between Node.js and Rust never changes its output.

## Rust server: `reze_server`

### Stay framework-agnostic

`reze_server` MUST work with any Rust HTTP framework. Its core is sans-io: it takes a parsed
request and a context and returns a response. It owns no socket, no async runtime, and no HTTP
stack.

```rust
let response = app.handle(request, &ctx).await; // http::Response<impl Stream<Item = Bytes>>
```

- Types: only the `http` and `bytes` crates, which every major framework shares.
- Runtime: the core NEVER spawns tasks or starts timers. Loaders are plain `async fn`, so their
  futures run on tokio, smol, or any other executor. Timeouts and cancellation belong to the
  framework, for example `tower::timeout`.
- Context: `App<C>` is generic over the application's context type. The adapter extracts it from
  the framework's state.
- Streaming: the body is a `Stream<Item = Bytes>`, which every framework can send.

### Adapters

Adapters sit behind cargo features, so an application pays only for the one it enables:

| Feature | Frameworks                         |
| ------- | ---------------------------------- |
| `tower` | axum, hyper, poem, salvo           |
| `actix` | actix-web, which has its own types |

```rust
let app = generated::routes::App::new().blog_id(routes::blog_id);

let router = axum::Router::new()
    .fallback_service(reze_server::tower::service(app))
    .with_state(state);
```

### JavaScript runtimes

The JS server entry, the handler `createHandler` returns, is
`(request: Request) => Promise<Response>` over the web `Request`
and `Response` types, so it runs on Node.js, Bun, Deno, and edge runtimes without adapters in
the core.

## Caching and prerendering

### Static routes

A route is static when no loader of its segments reads `request`, `search`, `context`, or
`response`. The compiler infers it and prerenders the route at build time. `context` counts as
dynamic: the compiler cannot tell `context.db` from `context.user`.

A route opts in explicitly when its loaders read `context` for build-time data only:

```tsx file=src/routes/blog/[id].tsx
export const prerender = true;
export const entries = async () => (await cms.posts()).map((post) => ({ id: post.id }));
```

`entries` lists the params to prerender for a dynamic route. A dynamic route without `entries` is
rendered on request.

### Revalidate by time

`revalidate` sets how long a rendered page stays fresh:

```tsx
export const revalidate = 60;
```

It becomes `Cache-Control: s-maxage=60, stale-while-revalidate`, and the CDN regenerates the page.
Reze has no cache store of its own.

### Conditional requests

The server sends an `ETag` for every response that does not stream and answers `If-None-Match`
with `304`. A streamed response has no `ETag`.

## Observability

### Rust

`reze_server` emits `tracing` spans; the application chooses the subscriber:

| Span       | Fields                                   |
| ---------- | ---------------------------------------- |
| `request`  | Method, path, status, duration           |
| `match`    | Route pattern                            |
| `loader`   | Route, segment depth, deferred, duration |
| `render`   | Route, bytes, duration                   |
| `mutation` | Mutation id, outcome, duration           |

### JavaScript

`createHandler` takes two hooks:

```ts
export default createHandler({
  context: async (request) => ({ db }),
  onRequest: (request) => log.info(request.method, request.url),
  onError: (error, request) => log.error(error, request.url),
});
```

`onError` receives every error that becomes a `500`, with its real message: the page shows a
generic one in production, see [Catch route errors](routing.md#catch-route-errors-with-errored).

### Server-Timing

Both servers write loader and render durations into `Server-Timing`, so browser devtools show
them per request. It is on in development and opt-in in production, because it reveals timing to
every client.

### Client

The client adds no hooks of its own: `rz:chunkerror`, `rz:swap`, and the standard `error` event on
`window` cover failures and navigations.

## Development

Rust render code needs a `cargo` build, which takes seconds. Development therefore renders in JS:

- Vite renders every route in JS, also for routes whose production render is Rust. A template
  edit applies at once, without `cargo`.
- Rust loaders and mutations run in a separate process built with the `dev` feature of
  `reze_server`. It answers data as JSON, and Vite proxies loader and mutation requests to it.
  `cargo watch` restarts it when Rust code changes.
- A component edit rebuilds its chunk, and the page applies a fresh server render with the morph.
  Kept segments and module state survive.

The JS render and the Rust render MUST produce the same HTML. `reze check --render-diff` renders
every route with fixture data through both paths and compares the bytes; CI runs it on every
change.

## Diagnostics

| Code                        | Raised when                                                         |
| --------------------------- | ------------------------------------------------------------------- |
| `MUTATION_INPUT_UNTYPED`    | A `mutation` handler has no input annotation and no schema          |
| `MUTATION_INPUT_UNRESOLVED` | The input annotation uses a type the compiler does not lower        |
| `MUTATION_ID_COLLISION`     | Two mutations hash to the same id                                   |
| `MUTATION_FORM_FIELD`       | A bound `<form>` has a named field the input type does not declare  |
| `LOADER_RESULT_UNTYPED`     | A client chunk reads loader data, and the loader has no result type |
| `LOADER_PARAMS_TYPE`        | A params annotation names a key missing from the pattern            |
| `DATA_TYPE_UNSUPPORTED`     | Loader data contains a type with no wire encoding                   |
| `SERVER_HOLE_UNSUPPORTED`   | A server-path hole has no Rust lowering                             |

## Implementation status

The current implementation differs from this design in these points:

- A mutation's id is base-36 of the 32-bit FNV-1a hash of `path#export`.
- A form post without `_b` answers `303` back to the page. With the dispatcher the post carries
  `_b`, and the server answers with the HTML directly: the whole page, the `422` page, or, for a
  mutation with `revalidate`, the segment of the shallowest module of the chain whose pattern is in
  the list, with `rz-segment`. A layout's pattern is its prefix, so `"/"` names the root layout.
  When no module of the chain is listed, the answer is empty with `rz-segment: -1`, and the page
  keeps its HTML. `revalidate` takes the patterns of `routes.gen.d.ts`.
- In the client bundle a mutation is a stub whose `.call`, or a direct call, posts JSON with `_b`.
  The server answers `{ result, status, segment, html }`, narrowed like a form post, and the
  client morphs that segment before `.call` resolves. Client state in the morphed body starts again from the new state block,
  so a value a handler sets after `await` on that state is lost. A JSON post without `_b` answers
  `{ result }`. The result uses named JSON, and `MutationInputError` comes from
  `reze-js/server`, which client code imports from the client runtime.
- On its first run, a binding of `value`, `checked`, or `selected` over the server's nodes skips a
  field whose live value differs from its default, so input typed before the chunk loaded or kept
  by a morph survives.
- A form posted to a mutation without a generated form coercer, a schema-first mutation, answers
  `415`.
- JSON input encodes `Date` as an ISO string or a timestamp, `Map` as `[key, value]` pairs, `Set`
  as an array, `bigint` as a decimal string, and `Uint8Array` as base64.
- `DATA_TYPE_UNSUPPORTED` covers reads of `props.data` in the route component's client code: a
  read of a `File`, or of a result annotation that does not lower.
- Positional encoding applies to the values of a state entry whose type client code knows, from
  a props annotation or the loader result. A whole object read encodes all its fields, not only
  those the chunk reads, and trailing absent optional fields are written as `null`, not dropped.
  The server wraps the value in an inline encoder, and client code rebuilds a named object at the
  read site instead of rewriting each later read into an index access. A value whose type no
  annotation gives crosses as named JSON.
- `LOADER_RESULT_UNTYPED` covers reads of `props.data` in the route component's client code only.
- `useSubmission` returns `{}` when the request did not submit that mutation, and `issues` holds the
  first issue for each path joined with `.`.
- The body limits are 1 MB for JSON and form fields and 10 MB for files, set through
  `createHandler({ limits })`.
- In development, a `500` carries the error message. In production it carries
  `Internal Server Error`.
- The dispatcher script is on every page, not only on pages with handler sites, so that links and
  server-tier forms navigate on the client. It loads the navigation module and the morph on intent
  over a link or a server-tier form.
- A partial render's `<head>` holds only the entries of the segments it renders. When they set no
  title, the page keeps the current one.
- A prefetched response is keyed by its URL and depth. The next route's chunk is preloaded by the
  idle preload, and only when a handler site is in the viewport.
- A `<Loading>` boundary is marked with the comments `<!--l0-->…<!--/l-->` around its fallback,
  not with a `data-rz-l` attribute, so it needs no wrapper element. Each streamed template holds the
  state block of its part, which the `stream` module adds once the dispatcher has started.
- A deferred field that rejects leaves the fallback in place and reports the error to `onError`.
- The JS handler takes a second argument, `hints(links)`, which it calls with the `Link` values of
  `103 Early Hints` for a document request before guards and loaders run. The Node middleware of
  `vite dev` and `vite preview` sends them with `writeEarlyHints`. The app has no stylesheets or
  fonts of its own, so the hints hold only the dispatcher as `modulepreload`; route chunks load on
  intent and are not hinted. The Rust server sends no hints: hyper has no `103` response.
- A route is static when every loader of its chain destructures only `params` and no layout has a
  guard. A page opts out with `prerender = false`. The build renders each static URL with the
  request header `rz-prerender`, which awaits deferred fields, and writes a `200` page to
  `<client outDir>/<path>/index.html` for the static host to serve.
- `revalidate` takes the smallest value of the chain. The `ETag` is a SHA-1 of the document, sent
  on `200` responses to `GET` and `HEAD` that do not stream.
- The Rust server renders loader data only: Rust has no mutations, deferred fields, or
  streaming. A Rust loader stops a request with a `RouteError`, and redirects with
  `RouteError::redirect(location)`, a `307` whose `status` may be changed; the dev endpoint
  answers the redirect with its status and `Location`, which the JS server answers with the same redirect. A `POST` answers
  `404`, or `403` when it is cross-site. `handle` takes any request body and returns
  `http::Response<Bytes>`; the `tower` service answers `Full<Bytes>`.
- A Rust guard is `export const guard = guardRoute.external()`, set with the generated
  `guard_<route>`: `async fn(Params, &http::request::Parts, &C) -> Result<()>`. It sees the
  request's URI and headers but not its body. The JS dev server asks the Rust `dev` server through
  `/__reze/guard` with the page URL and the request headers, which answers `204` when it passes.
- `reze_server::tower::service(app, context)` takes the context as an argument instead of
  extracting it from the framework's state. `reze_server::actix::service(Arc<App<C>>)` returns a
  `Route` for `default_service`, which reads the context from the app's `web::Data<C>`; each
  worker of `HttpServer` calls it with a clone of the `Arc`. Both answer with the whole body.
- The dev JSON of a Rust loader uses named fields. The Rust state block writes a typed value
  with `reze::Data::wire`, positionally, and the compiler reads the types of
  `virtual:reze-types/<crate>` from the IR. Params reach
  a Rust loader as strings, in the generated `Params` of the route.
- The IR has no `Map` or `Date`: a field of another type fails the build with "has no wire
  encoding". The `reze-ir` binary of `reze_build` writes the IR in place of `cargo xtask reze-ir`,
  and the Vite plugin reads it through `reze({ rust: { types } })`.
- The generated code renders one function per route level into `Html` and a `Render` state, and
  `reze_build::compile` runs the compiler itself in `build.rs`. The server reads the client URLs
  from `dist/server/reze.json` at startup, through `App::manifest`.
- Development has no Vite proxy: the JS handler fetches external loaders from the Rust `dev` server
  at `/__reze/load`, through `createHandler({ loaders })`.
- `Server-Timing` holds one `loader-<depth>` metric for each loader that ran and one `render`
  metric. `createHandler({ serverTiming })` and `App::server_timing` turn it on in production.
