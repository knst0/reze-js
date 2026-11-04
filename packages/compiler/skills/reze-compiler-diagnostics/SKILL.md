---
name: reze-compiler-diagnostics
description: Repair guide for reze-js compiler diagnostics. Use when a build, dev server or test prints a bracketed reze code such as [JSX_CONDITIONAL_EXPRESSION].
---

# reze-js compiler diagnostics

<!-- Generated from crates/reze_compiler/src/diagnostic/catalog.rs. Do not edit by hand: regenerate with `cargo xtask skill`. -->

Every diagnostic starts with its code in brackets, `[CODE]`, and says what was seen, why it is wrong and what to do.

## How to act

1. When the diagnostic has `fixes`, apply them: each fix is a list of exact byte edits (`start`, `end`, `text`) into the source that removes the diagnostic.
2. Otherwise follow the **Repair** of the code below.
3. Never suppress, silence or work around an `error` or a `warn`; fix its cause.
4. An `info` explains what the compiler did; it needs no action.

## Reading the `in` path

The `in` line lists what encloses the reported code, outermost first: components as `<Name>` and native elements by tag, for example `in <App> › <TodoList> › li › button`. Start at the last entry.

## Codes

## PARSE_ERROR

**The file does not parse** · severity `error`

> The parser rejected this file ({detail}). Fix the syntax at this position.

`data` keys: `detail`

Automatic fix: no

The parser reported a syntax error. `data.detail` is the parser's own message. Nothing of the project was compiled.

**Repair:** Fix the syntax at the reported position. JSX in a `.ts` file needs the `.tsx` extension.

Before:

```tsx
export default function Page() {
  return <main>;
}
```

After:

```tsx
export default function Page() {
  return <main />;
}
```

## ROUTE_INVALID

**The route file does not map to a URL** · severity `error`

> This route file cannot be served: {detail}.

`data` keys: `detail`

Automatic fix: no

Route files map to URL patterns by their names: `index`, `[id]`, `[[opt]]`, `[...rest]`, and `(group)` folders. Two files that match the same URL, a catch-all that is not last, or a param name that is not an identifier make the route table ambiguous. `data.detail` names the problem.

**Repair:** Rename or move the file so every URL has exactly one route.

Example: `src/routes/blog/index.tsx` and `src/routes/blog.tsx` without children both match `/blog`.

## IMPORT_UNRESOLVED

**A relative import names no file** · severity `error`

> `{specifier}` does not resolve to a file of the project.

`data` keys: `specifier`

Automatic fix: no

The compiler follows every relative import from the route modules to analyze the whole application. It tries the path as written, with `.tsx`, `.ts`, `.jsx`, and `.js`, and as a folder with an `index` file. `data.specifier` is the import as written.

**Repair:** Fix the path, or create the module.

Example:

```tsx
import { Card } from "./missing";

export default function Page() {
  return <Card />;
}
```

## ROUTE_DEFAULT_EXPORT

**A route module exports no component** · severity `error`

> This route module has no default export component, so the route renders nothing. Export the page component as `default`.

`data` keys: none

Automatic fix: no

Every file under the routes directory is a route module. The server renders its default export: the page, or the layout for files with nested routes.

**Repair:** Add `export default function Page() { … }`, or move the file out of the routes directory, or prefix its name with `_`.

Before:

```tsx
export function Page() {
  return <main />;
}
```

After:

```tsx
export default function Page() {
  return <main />;
}
```

## JSX_CONDITIONAL_EXPRESSION

**A condition renders JSX** · severity `error`

> A condition that renders JSX uses `<Show>` or `<Switch>`, not `{operator}`.

`data` keys: `operator`

Automatic fix: yes

Conditions that produce JSX are written with the control-flow components, so every target lowers them the same way, the callback narrows the value, and `&&` cannot render a stray `0`. `data.operator` is the operator found.

**Repair:** Apply the fix, or rewrite the expression as `<Show when={…} fallback={…}>…</Show>`.

Before:

```tsx
export default function Page(props: { user?: string }) {
  return <main>{props.user ? <p>{props.user}</p> : <p>Guest</p>}</main>;
}
```

After:

```tsx
import { Show } from "reze-js";

export default function Page(props: { user?: string }) {
  return (
    <main>
      <Show when={props.user} fallback={<p>Guest</p>}>
        {(user) => <p>{user}</p>}
      </Show>
    </main>
  );
}
```

## JSX_LIST_EXPRESSION

**An array method renders JSX** · severity `error`

> A list that renders JSX uses `<For>` or `<Repeat>`, not `.{method}`.

`data` keys: `method`

Automatic fix: yes

Lists that produce JSX are written with `<For>` or `<Repeat>`, which state how rows are keyed. `data.method` is the array method found.

**Repair:** Apply the fix, or rewrite the expression as `<For each={…}>{(item) => …}</For>`.

Before:

```tsx
export default function Page(props: { tags: string[] }) {
  return <ul>{props.tags.map((tag) => <li>{tag}</li>)}</ul>;
}
```

After:

```tsx
import { For } from "reze-js";

export default function Page(props: { tags: string[] }) {
  return (
    <ul>
      <For each={props.tags}>{(tag) => <li>{tag}</li>}</For>
    </ul>
  );
}
```

## JSX_ARRAY_METHOD

**A template calls an array method** · severity `error`

> Array methods are not called in templates: `.{method}` here. Compute the value in the loader, or in a `computed` for client state.

`data` keys: `method`

Automatic fix: no

The template stays a projection of data, so every target, the Rust server included, can lower it. `.length` is a property and stays allowed. `data.method` is the method called.

**Repair:** Move the computation into the loader and read the prepared field, or declare a `computed` for values derived from client state.

Before:

```tsx
export default function Page(props: { tags: string[] }) {
  return <p>{props.tags.join(", ")}</p>;
}
```

After:

```tsx
export default function Page(props: { tagList: string }) {
  return <p>{props.tagList}</p>;
}
```

## JSX_OUTSIDE_TEMPLATE

**JSX outside a component's returned template** · severity `error`

> JSX is written only in the template a component returns, in props of components, and in control-flow callbacks.

`data` keys: none

Automatic fix: no

A component runs on the server only, and its template is compiled into server render code and client bindings. JSX stored in a variable or returned from a helper function has no fixed place in that template.

**Repair:** Move the markup into the returned template, or extract it into a component.

Before:

```tsx
export default function Page() {
  const title = <h1>Hi</h1>;
  return <main>{title}</main>;
}
```

After:

```tsx
function Title() {
  return <h1>Hi</h1>;
}

export default function Page() {
  return (
    <main>
      <Title />
    </main>
  );
}
```

## COMPONENT_RETURN

**A component returns more than one template** · severity `error`

> A component returns one template, at the end of its body. Write conditions with `<Show>` or `<Switch>` inside it.

`data` keys: none

Automatic fix: no

The compiler gives every component one fixed template, so client bindings can locate their nodes by path. An early `return` makes the structure depend on runtime values.

**Repair:** Remove the early return and wrap the alternatives in `<Show>` or `<Switch>`.

Before:

```tsx
export default function Page(props: { ok: boolean }) {
  if (!props.ok) return <p>No</p>;
  return <p>Yes</p>;
}
```

After:

```tsx
import { Show } from "reze-js";

export default function Page(props: { ok: boolean }) {
  return (
    <Show when={props.ok} fallback={<p>No</p>}>
      <p>Yes</p>
    </Show>
  );
}
```

## CONTROL_FLOW_ATTRIBUTE

**Attribute a control-flow component does not accept** · severity `error`

> `<{tag}>` does not accept `{attribute}`. Remove it.

`data` keys: `attribute`, `tag`

Automatic fix: no

Control-flow components accept a fixed set of attributes: `<Show when fallback>`, `<Switch fallback>`, `<Match when>`, `<For each key fallback>`, `<Repeat count fallback>`, `<Loading fallback>`, `<Errored fallback>`, `<Portal mount>`. `data.tag` is the component and `data.attribute` the attribute.

**Repair:** Remove the attribute.

Before:

```tsx
import { Show } from "reze-js";

export default function Page(props: { ok: boolean }) {
  return <Show when={props.ok} each={1}><p>ok</p></Show>;
}
```

After:

```tsx
import { Show } from "reze-js";

export default function Page(props: { ok: boolean }) {
  return <Show when={props.ok}><p>ok</p></Show>;
}
```

## CONTROL_FLOW_MISSING

**A control-flow component lacks a required attribute** · severity `error`

> `<{tag}>` needs `{attribute}`.

`data` keys: `attribute`, `tag`

Automatic fix: no

`<Show>` and `<Match>` need `when`, `<For>` needs `each`, `<Repeat>` needs `count`. `data.tag` is the component and `data.attribute` the missing attribute.

**Repair:** Add the attribute.

Before:

```tsx
import { Show } from "reze-js";

export default function Page() {
  return <Show><p>ok</p></Show>;
}
```

After:

```tsx
import { Show } from "reze-js";

export default function Page(props: { ok: boolean }) {
  return <Show when={props.ok}><p>ok</p></Show>;
}
```

## CONTROL_FLOW_CHILDREN

**Children a control-flow component cannot render** · severity `error`

> `<{tag}>` takes {expected} as children.

`data` keys: `expected`, `tag`

Automatic fix: no

`<For>` and `<Repeat>` take one callback that renders a row. `<Show>` takes markup or one callback that receives the narrowed value. `<Switch>` takes only `<Match>` elements. `data.tag` is the component and `data.expected` what it accepts.

**Repair:** Write the children in the expected form.

Before:

```tsx
import { For } from "reze-js";

export default function Page(props: { tags: string[] }) {
  return <ul><For each={props.tags}><li /></For></ul>;
}
```

After:

```tsx
import { For } from "reze-js";

export default function Page(props: { tags: string[] }) {
  return <ul><For each={props.tags}>{(tag) => <li>{tag}</li>}</For></ul>;
}
```

## CONTROL_FLOW_AS_VALUE

**A compiler component used as a value** · severity `error`

> `{name}` exists only as a JSX tag. Use it as `<{name}>`.

`data` keys: `name`

Automatic fix: no

`Show`, `Switch`, `Match`, `For`, `Repeat`, `Loading`, `Errored`, and `Portal` are compiled away and have no runtime value. `data.name` is the component.

**Repair:** Render it as a tag.

Before:

```tsx
import { Show } from "reze-js";

export const Conditional = Show;
```

After:

```tsx
import { Show } from "reze-js";

export default function Page(props: { ok: boolean }) {
  return <Show when={props.ok}><p>ok</p></Show>;
}
```

## UNKNOWN_COMPONENT

**A component the compiler cannot find** · severity `error`

> `<{name}>` does not resolve to a component declared in the project.

`data` keys: `name`

Automatic fix: no

The compiler analyzes every component it renders: their templates become server render code and client bindings. A tag must name a component declared in the project or imported from a module with source. `data.name` is the tag.

**Repair:** Import the component from a project module, or declare it.

Before:

```tsx
export default function Page() {
  return <Missing />;
}
```

After:

```tsx
function Present() {
  return <p />;
}

export default function Page() {
  return <Present />;
}
```

## PACKAGE_SOURCE_MISSING

**A component from a package without source** · severity `error`

> `{package}` exposes no `reze` source, so `{name}` cannot be compiled.

`data` keys: `name`, `package`

Automatic fix: no

The compiler analyzes a library component like project code, so a package publishes its source through the `reze` export condition. `data.package` is the package and `data.name` the component.

**Repair:** Use a package with a `reze` export, or write the component in the project.

Before:

```tsx
import { Dialog } from "some-ui";

export default function Page() {
  return <Dialog />;
}
```

After:

```tsx
function Dialog() {
  return <dialog />;
}

export default function Page() {
  return <Dialog />;
}
```

## SPREAD_CHILD

**A spread child in JSX** · severity `error`

> `{...}` as a JSX child has no fixed place in the template. Render lists with `<For>`.

`data` keys: none

Automatic fix: no

A spread child inserts an unknown number of nodes, which no target can locate.

**Repair:** Use `<For each={…}>`.

Before:

```tsx
export default function Page(props: { items: string[] }) {
  return <p>{...props.items}</p>;
}
```

After:

```tsx
import { For } from "reze-js";

export default function Page(props: { items: string[] }) {
  return <p><For each={props.items}>{(item) => item}</For></p>;
}
```

## SERVER_HOLE_UNSUPPORTED

**A template hole the server cannot lower** · severity `error`

> {reason}. Compute the value in the loader, or make the element interactive on the client.

`data` keys: `reason`

Automatic fix: no

A hole rendered on the server is a projection of params, data, context, and loop variables: field access, literals, template literals, logical and comparison operators, arithmetic, and `.length`. Calls to unanalyzed functions have no Rust lowering, and the JS server and the Rust server must render the same bytes. `data.reason` names the expression.

**Repair:** Compute the value in the loader and read the prepared field.

Before:

```tsx
export default function Page(props: { at: Date }) {
  return <time>{props.at.toISOString()}</time>;
}
```

After:

```tsx
export default function Page(props: { at: string }) {
  return <time>{props.at}</time>;
}
```

## RUST_ROUTE_INVALID

**The Rust file of a route does not fit the contract** · severity `error`

> {reason}.

`data` keys: `reason`

Automatic fix: no

A `.rs` file beside a route module holds its server code: `pub async fn load(params: Params, cx: &Context) -> Result<T>`, `pub async fn guard(params: Params, request: &Parts, cx: &Context) -> Result<()>`, and mutations `pub async fn name(input: I, request: &Parts, cx: &Context) -> Result<T>`. Data types derive `reze::Data`, are `pub`, have names unique in the crate, and live in the route file or under `src/server.rs`. The file holds items only, since reze includes it into the generated crate. `data.reason` names the problem.

**Repair:** Match the signature of the contract, or move the code into the module tree of `src/server.rs`.

Example: `pub async fn load(id: u32) -> Post` in `src/routes/posts/[id].rs`, which takes no `Params` and returns no `Result`.

## SERVER_JS_IN_RUST

**JS server code in an application whose server is Rust** · severity `error`

> The Rust server cannot run the JS `{name}`: write it as `pub async fn {function}` in {file}.

`data` keys: `file`, `function`, `name`

Automatic fix: no

A project with a `Cargo.toml` at the Vite root builds one server, in Rust, and that server runs no JS. Every loader, guard, and mutation is a `pub async fn` of the `.rs` file beside its route module, and the route module imports its mutations from there. `data.name` is the JS export, `data.function` the Rust function, and `data.file` the file it belongs in.

**Repair:** Move the function into the `.rs` file beside the route module, or remove `Cargo.toml` to build the JS server.

Example: `export const load = loader(…)` in `src/routes/index.tsx` of a project with a `Cargo.toml`, instead of `pub async fn load` in `src/routes/index.rs`.

## HANDLER_CAPTURE_UNSERIALIZABLE

**A handler captures a value that cannot reach the browser** · severity `error`

> `{name}` is {what}, which cannot cross from server to browser.

`data` keys: `name`, `what`

Automatic fix: no

A handler runs in the browser without its component, so every value it captures reaches the browser in the client state: signals and stores as references, data as positional JSON, project functions as code. Anything else, such as class instances or values from unanalyzed calls, cannot be encoded. `data.name` is the captured variable and `data.what` what it holds.

**Repair:** Create the value inside the handler or inside an `effect`.

Before:

```tsx
export default function Page() {
  const socket = new WebSocket("/live");
  return <button onClick={() => socket.send("ping")}>Ping</button>;
}
```

After:

```tsx
export default function Page() {
  return <button onClick={() => new WebSocket("/live").send("ping")}>Ping</button>;
}
```

## MODULE_STATE_SERVER_WRITE

**The server render writes module state** · severity `error`

> `{name}` is module state, and the server render writes it here. Write it from a handler or an `effect`.

`data` keys: `name`

Automatic fix: no

A server process serves many users, so a write during the server render would leak between requests. Module state is read-only on the server, and its initializer is a constant. `data.name` is the state.

**Repair:** Move the write into a handler or an `effect`, or keep the value in loader data.

Before:

```tsx
import { signal } from "reze-js";

let count = signal(0);

export default function Page() {
  count = 1;
  return <p>{count}</p>;
}
```

After:

```tsx
import { signal } from "reze-js";

let count = signal(0);

export default function Page() {
  return <button onClick={() => count++}>{count}</button>;
}
```

## MODULE_STATE_INITIALIZER

**Module state starts from a non-constant value** · severity `error`

> The initializer of module state `{name}` must be a constant, because the server renders it and the state block carries nothing for it.

`data` keys: `name`

Automatic fix: no

Module state has one instance in the browser and is read-only on the server. Its initial value must be the same in every request and every build, so the compiler requires a literal. `data.name` is the state.

**Repair:** Initialize it with a literal, and write the real value from a handler, or use loader data.

Before:

```tsx
import { signal } from "reze-js";

export let now = signal(Date.now());
```

After:

```tsx
import { signal } from "reze-js";

export let now = signal(0);
```

## EVENT_DECISION_DEFERRED

**An event decision that cannot wait for the chunk** · severity `error`

> `{method}` depends on {condition}, which the dispatcher cannot evaluate before the chunk loads.

`data` keys: `condition`, `method`

Automatic fix: no

The handler's chunk loads when the user starts the interaction, so the dispatcher decides `preventDefault`, `stopPropagation`, and `stopImmediatePropagation` from markup. Unconditional calls and conditions on `key`, `code`, `button`, and the modifier flags are extracted into the markup; any other condition is not. `data.method` is the call; `data.condition` is the test it depends on, or `control flow`.

**Repair:** Move the check to a native mechanism, such as `required` or a `beforeunload` guard, or set `eager` on the element so its chunk loads before the first interaction.

Before:

```tsx
import { signal } from "reze-js";

export default function Page() {
  let dirty = signal(false);
  return <a href="/next" onClick={(event) => { if (dirty) event.preventDefault(); }}>Next</a>;
}
```

After:

```tsx
export default function Page() {
  return <a href="/next" onClick={(event) => event.preventDefault()}>Next</a>;
}
```

## DYNAMIC_COMPONENT_UNBOUNDED

**The candidates of `dynamic` are not known** · severity `error`

> The candidates of `dynamic` are not known at build time. Choose from an object literal of components.

`data` keys: none

Automatic fix: no

Every target renders each candidate of a dynamic component and switches between them, so the set must be finite and visible to the compiler: an object literal whose values are project components.

**Repair:** Choose from an object, `dynamic(icons[props.kind])` with `const icons = { star: Star }`.

Before:

```tsx
import { dynamic } from "reze-js";

export default function Page(props: { c: any }) {
  const Widget = dynamic(props.c);
  return <Widget />;
}
```

After:

```tsx
import { dynamic } from "reze-js";

function A() {
  return <p>a</p>;
}

const parts = { a: A };

export default function Page(props: { c: "a" }) {
  const Widget = dynamic(parts[props.c]);
  return <Widget />;
}
```

## MUTATION_INPUT_UNTYPED

**A mutation without an input contract** · severity `error`

> A mutation is a public endpoint and needs a contract for its input. Annotate the input, or pass a schema.

`data` keys: none

Automatic fix: no

Every `mutation` is a public POST endpoint, so its input is validated at runtime. The compiler generates the validator from the input annotation; a Standard Schema passed as the first argument replaces it. `unknown`, `any`, and a missing annotation describe nothing.

**Repair:** Annotate the first parameter with the input type, or pass a schema.

Before:

```tsx
import { mutation } from "reze-js/server";

export const save = mutation(async (input) => input);
```

After:

```tsx
import { mutation } from "reze-js/server";

export const save = mutation(async (input: { title: string }) => input.title);
```

## MUTATION_INPUT_UNRESOLVED

**A data type the compiler does not lower** · severity `error`

> {reason}. Write the type out, or pass a schema.

`data` keys: `reason`

Automatic fix: no

The compiler lowers annotations syntactically: primitives, literals and their unions, object types, interfaces and aliases from project files, arrays, tuples, `Record`, `Map`, `Set`, `Date`, `Uint8Array`, `File`, discriminated unions, and the integer aliases of `reze-js/types`. `data.reason` names what it could not lower.

**Repair:** Write the object type out, use a supported type, or pass a schema.

Before:

```tsx
import { mutation } from "reze-js/server";

interface Todo { title: string; done: boolean }

export const save = mutation(async (input: Pick<Todo, "title">) => input.title);
```

After:

```tsx
import { mutation } from "reze-js/server";

export const save = mutation(async (input: { title: string }) => input.title);
```

## MUTATION_ID_COLLISION

**Two mutations share an endpoint id** · severity `error`

> `{name}` and `{other}` hash to the same mutation id `{id}`. Rename one of them.

`data` keys: `id`, `name`, `other`

Automatic fix: no

A mutation id is base-36 of a hash of its module path and export name, so it stays the same across builds. Two mutations with the same id would share an endpoint. `data.name` and `data.other` are the mutations and `data.id` the id.

**Repair:** Rename the export or the file of one mutation.

Example: Two mutations in different files whose path and export name hash to the same id.

## MUTATION_FORM_FIELD

**A form field the mutation input does not declare** · severity `error`

> The form posts `{field}`, which the input type of `{mutation}` does not declare.

`data` keys: `field`, `mutation`

Automatic fix: no

A `<form action={mutation}>` posts its named fields as the mutation input. A field the input type does not declare would be rejected or ignored. `data.field` is the field and `data.mutation` the mutation.

**Repair:** Add the field to the input type, or rename the field.

Before:

```tsx
import { mutation } from "reze-js/server";

export const save = mutation(async (input: { title: string }) => input.title);

export default function Page() {
  return <form action={save}><input name="name" /></form>;
}
```

After:

```tsx
import { mutation } from "reze-js/server";

export const save = mutation(async (input: { title: string }) => input.title);

export default function Page() {
  return <form action={save}><input name="title" /></form>;
}
```

## LOADER_RESULT_UNTYPED

**Client code reads loader data without a result type** · severity `error`

> The client reads `{read}`, but the loader of `{route}` has no result type.

`data` keys: `read`, `route`

Automatic fix: no

Client state crosses the wire as positional JSON, which needs a type the compiler can lower. The compiler never asks the TypeScript checker for an inferred type. `data.read` is the read and `data.route` the route pattern.

**Repair:** Annotate the result, `loader<Post>(…)` or `async (): Promise<Post> => …`.

Before:

```tsx
import { signal } from "reze-js";
import { loader } from "reze-js/server";

export const load = loader(async () => ({ likes: 1 }));

export default function Page(props: { data: { likes: number } }) {
  let likes = signal(props.data.likes);
  return <button onClick={() => likes++}>{likes}</button>;
}
```

After:

```tsx
import { signal } from "reze-js";
import { loader } from "reze-js/server";

interface Post { likes: number }

export const load = loader<Post>(async () => ({ likes: 1 }));

export default function Page(props: { data: Post }) {
  let likes = signal(props.data.likes);
  return <button onClick={() => likes++}>{likes}</button>;
}
```

## LOADER_PARAMS_TYPE

**A params annotation names a key the pattern lacks** · severity `error`

> The params annotation declares `{name}`, which the pattern `{route}` does not have.

`data` keys: `name`, `route`

Automatic fix: no

Params come from the URL pattern of the route. An annotation can change their type but not add keys. `data.name` is the key and `data.route` the pattern.

**Repair:** Remove the key, or rename the route file to add the param.

Example: `loader(async ({ params }: { params: { slug: string } }) => …)` in `src/routes/blog/[id].tsx`.

## DATA_TYPE_UNSUPPORTED

**Data with no wire encoding** · severity `error`

> Client code reads `{read}`, whose type has no wire encoding: {reason}.

`data` keys: `read`, `reason`

Automatic fix: no

Values that reach the browser are encoded as positional JSON, which carries primitives, literals, objects, arrays, maps, sets, dates, and bytes. `data.read` is the read, and `data.reason` names the type.

**Repair:** Convert the value in the loader to a supported type.

Example: A loader annotated `loader<{ at: Temporal.Instant }>` whose field a handler reads.

## SERVER_IN_SPA

**Server code in a client-only application** · severity `error`

> `target: "spa"` has no server to run `{name}`.

`data` keys: `name`

Automatic fix: no

A client-only application is a static `index.html` and one chunk per route. Loaders and mutations need a server. `data.name` is the export.

**Repair:** Build with a server target, or fetch the data in an `effect`.

Example: `export const load = loader(…)` in an application built with `reze({ target: "spa" })`.

## CLIENT_RANGE_UNSUPPORTED

**Content client code cannot create** · severity `error`

> Client code cannot create {what} inside a range it updates.

`data` keys: `what`

Automatic fix: no

A `<Show>`, `<Switch>`, `<For>`, or `<Repeat>` whose condition or source reads client state is rendered again in the browser. Client code creates its content from the template: elements, text, bindings, handlers, and nested ranges. Components, boundaries, `<head>`, `<Portal>`, `children`, spread attributes, computed form actions, and callback bodies with statements are rendered only on the server. `data.what` names the construct.

**Repair:** Move the construct outside the range, or inline the component's markup into the range.

Before:

```tsx
import { signal, For } from "reze-js";
import { Item } from "./item";

export default function Page() {
  let items = signal(["a"]);
  return (
    <ul onClick={() => (items = [...items, "b"])}>
      <For each={items}>{(item) => <Item name={item} />}</For>
    </ul>
  );
}
```

After:

```tsx
import { signal, For } from "reze-js";

export default function Page() {
  let items = signal(["a"]);
  return (
    <ul onClick={() => (items = [...items, "b"])}>
      <For each={items}>{(item) => <li>{item}</li>}</For>
    </ul>
  );
}
```

## UNTRACK_REDUNDANT

**An `untrack` the compiler does not need** · severity `warn`

> This read is already untracked here; remove `untrack`.

`data` keys: none

Automatic fix: yes

The compiler decides which reads subscribe: reads inside handlers and reads that only initialize state never do. `untrack` is an escape hatch for the reads a rule does not cover.

**Repair:** Replace `untrack(() => x)` with `x`.

Before:

```tsx
import { signal, untrack } from "reze-js";

export default function Page() {
  let count = signal(0);
  return <button onClick={() => console.log(untrack(() => count))}>Log</button>;
}
```

After:

```tsx
import { signal } from "reze-js";

export default function Page() {
  let count = signal(0);
  return <button onClick={() => console.log(count)}>Log</button>;
}
```
