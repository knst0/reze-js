# Public API

Status: draft. The user-facing API of Reze, designed from scratch for the model in
[Server rendering](ssr.md) and [Interactivity](interactivity.md). Sections marked
**Decided** are agreed. Rules use MUST, NEVER, and SHOULD in the RFC 2119 sense.

## Principles

- The API is compiler syntax. Every API MUST lower to the client target, the JS server target, and
  the Rust server target, or be refused with a diagnostic.
- A component function runs on the server only. The browser receives handlers and the bindings
  they update, never the component body. An API that needs a component to run in the browser
  does not exist.
- No runtime helper exists for what the compiler can express as syntax.

## What a handler captures

**Decided** by the model. A handler runs in the browser without its component, so every value it
captures from the component reaches the browser in the client state:

```tsx
function Cart(props: { items: Item[] }) {
  let count = signal(0);
  const limit = props.items.length;
  const format = (n: number) => `${n} pcs`;

  return (
    <button
      onClick={() => {
        if (count < limit) count++;
        console.log(format(count));
      }}
    >
      Add
    </button>
  );
}
```

| Captured value                    | How it reaches the browser                                            |
| --------------------------------- | --------------------------------------------------------------------- |
| `signal`, `store`                 | A reference into the shared client state, one identity for all chunks |
| Value from props, data, constants | Encoded as positional JSON                                            |
| Function declared in the project  | Code in the chunk; its own captures follow these same rules           |
| Anything else                     | Error                                                                 |

```tsx
function Live() {
  const socket = new WebSocket("/live");
  return <button onClick={() => socket.send("ping")}>Ping</button>;
}
// HANDLER_CAPTURE_UNSERIALIZABLE: `socket` is a `WebSocket`, which cannot cross from server to
// browser.
// Fix: create it inside the handler or inside an `effect`.
```

## Reactivity

**Decided.** `signal`, `computed`, `store`, and `action` are the reactive core:

```tsx
let count = signal(0);
const doubled = computed(count * 2);
const todos = store<Todo[]>([]);
const addTodo = action((title: string) => {
  todos.push({ title, done: false });
  count++;
});
```

`effect` runs code that needs the browser without a user action. Its chunk loads when its element
enters the viewport. `onCleanup` runs when the DOM range that owns the effect is removed:

```tsx
function Clock() {
  let now = signal(Date.now());
  effect(() => {
    const id = setInterval(() => (now = Date.now()), 1000);
    onCleanup(() => clearInterval(id));
  });
  return <time>{new Date(now).toLocaleTimeString()}</time>;
}
```

### Untracked reads

The compiler decides which reads subscribe. A read inside a handler, an `effect` setup that only
needs the initial value, or a value used only to initialize state does not subscribe, without any
marker. `untrack` stays as an escape hatch for the cases a rule does not cover:

```tsx
effect(() => {
  analytics.track("view", { page: currentPage, user: untrack(() => user.id) });
});
```

The effect reruns when `currentPage` changes, not when `user.id` does. Every `untrack` that the
compiler can prove redundant is reported, so the escape hatch does not spread.

## Ownership

**Decided.** The API has no ownership primitives: no `getOwner`, `runWithOwner`, `root`, or
`effectScope`. An owner tree only exists when components run in the browser, and here they do not.
The compiler assigns every effect and cleanup to the DOM range that contains it.

## Events and refs

**Decided.** Handlers are JSX attributes, as in the DOM:

```tsx
<input onInput={(event) => (query = event.currentTarget.value)} />
```

A `ref` receives the element in the browser, typed from the tag: `HTMLCanvasElement` for
`<canvas>`, `HTMLInputElement` for `<input>`. Like `effect`, it loads its chunk when the element
enters the viewport:

```tsx
<canvas
  ref={(canvas) => {
    const context = canvas.getContext("2d");
    if (context) draw(context);
  }}
/>
```

## Context

**Decided.** `createContext` and `useContext` keep their meaning. A context value that client code
reads follows the [capture rule](#what-a-handler-captures):

```tsx
const Theme = createContext<"light" | "dark">("light");

function Toggle() {
  const theme = useContext(Theme);
  return <button onClick={() => console.log(theme)}>Theme</button>;
}
```

`theme` is a string, so it is encoded as JSON. A context that holds a `store` passes its reference.
A context that holds a class instance is refused when a handler reads it.

## Control flow

**Decided.** Conditions and lists that produce JSX are written only with the control-flow
components: `Show`, `Switch` and `Match`, `For`, and `Repeat`.

```tsx
<Show when={user} fallback={<SignIn />}>
  {(user) => <Profile user={user} />}
</Show>

<Switch>
  <Match when={status === "loading"}><Spinner /></Match>
  <Match when={status === "error"}><Retry /></Match>
  <Match when={status === "ready"}><Feed /></Match>
</Switch>

<For each={todos} key="id">
  {(todo) => <TodoItem todo={todo} />}
</For>

<Repeat count={5}>{(index) => <SkeletonRow index={index} />}</Repeat>
```

A ternary, `&&`, `||`, or `.map` whose result is JSX is an error:

```tsx
<main>{user ? <Profile user={user} /> : <SignIn />}</main>
// JSX_CONDITIONAL_EXPRESSION: a condition that renders JSX uses `<Show>` or `<Switch>`.
// Fix: <Show when={user} fallback={<SignIn />}>{(user) => <Profile user={user} />}</Show>

<main>{count && <Badge count={count} />}</main>
// JSX_CONDITIONAL_EXPRESSION: same as above. `&&` would also render `0` when `count` is zero.

<main>{todos.map((todo) => <TodoItem todo={todo} />)}</main>
// JSX_LIST_EXPRESSION: a list that renders JSX uses `<For>` or `<Repeat>`.
// Fix: <For each={todos}>{(todo) => <TodoItem todo={todo} />}</For>
```

Each diagnostic carries an LSP quick fix that rewrites the expression into the component.

Expressions that produce values stay plain JS, in attributes and in text:

```tsx
<li class={todo.done ? "done" : "open"}>{todo.title}</li>
<span>{count === 1 ? "item" : "items"}</span>
```

### No array methods in templates

A JSX expression NEVER calls an array method, whether it produces JSX or a value. `.length` is a
property and stays allowed. Prepare the value in the loader:

```tsx
<span>{post.tags.map((tag) => tag.name).join(", ")}</span>
// JSX_ARRAY_METHOD: array methods are not called in templates.
// Fix: compute the value in the loader, or in a `computed` for client state.
```

```tsx
export const load = loader(async ({ params }) => {
  const post = await db.post(params.id);
  return { title: post.title, tagList: post.tags.map((tag) => tag.name).join(", ") };
});

export default function Post(props: RoutePropsFor<"/blog/:id">) {
  return <span>{props.data.tagList}</span>;
}
```

The template stays a projection of data, which keeps every target, the Rust server included,
able to lower it.

Why components only:

- One way to write a condition or a list.
- Semantics are explicit: keyed or by index in `For`, a count in `Repeat`, exhaustive branches in
  `Switch`. A `.map` would need its own rule for what an unkeyed list means.
- `Show` narrows its value in the callback: `user` is non-null inside without `!`.
- `&&` cannot render a stray `0`.
- The set is closed and small, so the Rust lowering and the diagnostics stay simple.

## Props

**Decided.** Destructuring and rest props are syntax the compiler reads:

```tsx
function Button(props: ButtonProps) {
  const { variant, ...rest } = props;
  return <button class={`btn btn-${variant}`} {...rest} />;
}
```

`mergeProps`, `splitProps`, and `omitProps` do not exist. A runtime helper hides keys from
analysis: the compiler loses track of which props are bindings, and the Rust target cannot lower
the call.

## Derived optimizations

**Decided.** The compiler infers what a user would otherwise optimize by hand. A comparison
against one shared signal inside a list lowers to a selector, so a change updates two rows, not
all of them:

```tsx
let selected = signal<number | undefined>(undefined);

<For each={rows} key="id">
  {(row) => (
    <tr class={row.id === selected ? "active" : ""} onClick={() => (selected = row.id)}>
      …
    </tr>
  )}
</For>;
```

`selector` is not part of the API.

## Async boundaries

**Decided.** Components do not fetch on the server: data comes from the route `loader`.
`<Loading>` wraps reads of deferred loader fields, which stream when they resolve:

```tsx
export const load = loader(async ({ params }) => ({
  post: await db.post(params.id),
  comments: defer(db.comments(params.id)),
}));

export default function Post(props: RoutePropsFor<"/blog/:id">) {
  return (
    <article>
      <h1>{props.data.post.title}</h1>
      <Loading fallback={<p>Loading comments…</p>}>
        <Comments list={props.data.comments} />
      </Loading>
    </article>
  );
}
```

Deferred parts need JS to appear, so defer only secondary content. [Streaming](ssr.md#streaming)
specifies the order and the markup.

`<Errored>` catches loader errors on the server and handler errors in the browser.

## Dynamic components

**Decided.** A component chosen at runtime is allowed when the candidates form a finite set the
compiler can see:

```tsx
const icons = { star: StarIcon, heart: HeartIcon };
const Icon = dynamic(icons[props.kind]);
```

Rust renders each candidate and switches between them. An unbounded source is an error:

```tsx
const Widget = dynamic(await import(props.path));
// DYNAMIC_COMPONENT_UNBOUNDED: the candidates of `dynamic` are not known at build time.
// Fix: choose from an object or a union of imported components.
```

## Document and utilities

**Decided.**

- A `<head>` element written in any component hoists its children into the document head. Tags
  with the same key replace each other, the deepest segment wins: `title`, `base`,
  `link[rel=canonical]`, and `meta` by `name`, `property`, or `http-equiv`. No `Head` component
  exists.

  ```tsx
  <head>
    <title>{props.data.title}</title>
  </head>
  ```

- `Portal` renders children elsewhere in the document.
- `createUniqueId` returns an id that is stable between the server HTML and the browser.
- `render` mounts an app in the browser, for `render: "client"` routes and client-only apps.

## Server

**Decided.** `loader` and `mutation` come from `reze-js/server`. [Server rendering](ssr.md)
specifies both.

**Decided.** Data caching (`cache`, tag revalidation) lives in `reze-js/server` next to them,
because it is a concern of loaders and mutations, not of components.

**Decided.** `useSubmission(mutation)` returns `{ input, issues, result }` of the current request,
and `.call` throws `MutationInputError` on invalid input. Both come from `reze-js/server`.
[Report invalid input](ssr.md#report-invalid-input) specifies them.

## Router

**Decided.**

- A route component reads `props.params` and `props.data`.
- Hooks (`useParams`, `useLocation`, `useSearchParams`) stay for any component that wants router
  state without receiving it through props, at any depth. On the server a hook returns a
  constant; in the browser, a value of the router's client state.
- Every `<a href>` with a routable path gets `aria-current="page"` and `data-active`
  automatically whenever the app has a router. No option turns it on. An anchor that sets
  `aria-current` itself keeps its own value.
- `useLinkState` serves components that render links the compiler cannot see, such as links
  inside a library component.
- `buildPaths` and typed `href` values keep links checked at build time.
- [Routing](routing.md) specifies files, segments, layout loaders, search params, redirects,
  and route errors.

## Component libraries

**Decided.** The compiler analyzes a library like project code, so a library publishes its source.
A package exposes TSX through the `reze` export condition:

```json
{
  "exports": {
    ".": { "reze": "./src/index.ts", "default": "./dist/index.js" }
  }
}
```

- A package with a `reze` export compiles with the project: the same rules, the same diagnostics,
  and its types resolve in annotations. The build caches it by package version.
- A component imported from a package without source is an error:

  ```tsx
  import { Dialog } from "some-ui";
  // PACKAGE_SOURCE_MISSING: `some-ui` exposes no `reze` source, so `Dialog` cannot be compiled.
  // Fix: use a package with a `reze` export, or write the component in the project.
  ```

- A plain JS library, such as `date-fns`, works as unanalyzed calls.

## Not in the API

| API                                                    | Status  | Reason                                                                                                                                                                   |
| ------------------------------------------------------ | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `getOwner`, `runWithOwner`, `root`, `effectScope`      | Decided | No owner tree exists in the browser                                                                                                                                      |
| `island`                                               | Decided | The compiler decides what runs in the browser. `eager` on an element overrides loading                                                                                   |
| `mergeProps`, `splitProps`, `omitProps`                | Decided | Hide keys from analysis; destructuring replaces them                                                                                                                     |
| `selector`                                             | Decided | Inferred by the compiler                                                                                                                                                 |
| `flush`                                                | Decided | Only tests need it; it belongs to the testing library                                                                                                                    |
| Plugins: wire codecs, components from other frameworks | Decided | The core semantics are fixed, so every page keeps the same guarantees. Components that depend on another framework's runtime, such as React libraries, are not supported |
| `trigger`, `readonly`, `catchError`                    | Decided | Keep only what no compiler rule can express; review each                                                                                                                 |

## Moved and renamed

| API                                 | Becomes                   | Reason                                                     |
| ----------------------------------- | ------------------------- | ---------------------------------------------------------- |
| `server`                            | `mutation`                | Pairs with `loader` as read and write; `action` stays free |
| `Head`                              | `<head>` in any component | The platform element; no wrapper to import                 |
| `cache`, `revalidateTag`            | `reze-js/server`          | A concern of loaders and mutations, not of components      |
| Ternary, `&&`, `\|\|` producing JSX | `<Show>`, `<Switch>`      | One form for conditions, no stray `0`, type narrowing      |
| `.map` producing JSX                | `<For>`, `<Repeat>`       | Explicit keying                                            |

## Diagnostics

| Code                             | Raised when                                                         |
| -------------------------------- | ------------------------------------------------------------------- |
| `HANDLER_CAPTURE_UNSERIALIZABLE` | A handler or binding captures a value that cannot reach the browser |
| `JSX_CONDITIONAL_EXPRESSION`     | A ternary, `&&`, or `\|\|` produces JSX                             |
| `JSX_LIST_EXPRESSION`            | `.map` or another array method produces JSX                         |
| `JSX_ARRAY_METHOD`               | A JSX expression calls an array method                              |
| `UNTRACK_REDUNDANT`              | An `untrack` wraps a read the compiler already leaves untracked     |
| `PACKAGE_SOURCE_MISSING`         | A component comes from a package without a `reze` export            |
| `DYNAMIC_COMPONENT_UNBOUNDED`    | The candidates of `dynamic` are not known at build time             |

## Implementation status

The current implementation differs from this design in these points:

- The compiler reads a package's `reze` source from `node_modules/<package>` under the project
  root, through the `.` or subpath entry of `exports`. The build compiles it each time instead
  of caching it by package version.
- The Vite plugin adds the `reze` condition to both environments, bundles the app's
  dependencies that declare it into the server build, and dedupes `reze-js`, so a package's
  compiled code imports the app's runtime.
- The quick fix rewrites a ternary and `&&` into `<Show>`, and `.map` with one arrow function into
  `<For>`, and adds the missing import. `||` and the other array methods get the diagnostic
  without a fix.
- Active links: the compiler marks an `<a>` whose `href` is a static path or an expression, and
  skips one with a spread or its own `aria-current`. After a client navigation the runtime moves
  the marks across every `<a href>` in the document, including ones the compiler did not see,
  and leaves an anchor alone that has `aria-current` without `data-active`.
- Of the router hooks, only `useParams` exists, and only in the server render. `useLocation`,
  `useSearchParams`, and `useLinkState` are not implemented.
- `buildPaths` and typed `href` values are not implemented; `routes.gen.d.ts` types route props
  only.
- `render` is not exported. `render: "client"` routes and `target: "spa"` mount through the
  navigation runtime instead.
