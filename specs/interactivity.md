# Interactivity

Status: draft for discussion. Design of how a server-rendered page becomes interactive. It
continues [Server rendering](ssr.md). Rules use MUST, NEVER, and SHOULD in the RFC 2119 sense.

## Goals

- The first response is a visually complete page that runs no JS.
- Interactivity appears when the user starts an action. The code for that action loads at that
  moment, or earlier when the browser can predict it.
- An interaction loads only its handler and the bindings that handler updates. Static nodes
  NEVER receive client code.
- The HTML carries only the metadata the client cannot derive by itself.

## Terms

| Term         | Meaning                                                                         |
| ------------ | ------------------------------------------------------------------------------- |
| Handler site | An element with an event handler                                                |
| Binding      | A DOM write that depends on client state: text, attribute, property, range      |
| Client chunk | A module with one or more handlers and the bindings they update                 |
| Intent       | An event that predicts an interaction: `pointerdown`, `pointerenter`, `focusin` |
| Dispatcher   | The small shared script that delegates events and loads client chunks           |
| Client state | The values that handlers and bindings read, sent from server to browser as JSON |

## Three tiers

The compiler places every interaction in the cheapest tier that keeps its behavior.

| Tier       | What runs                                                         | Client JS             |
| ---------- | ----------------------------------------------------------------- | --------------------- |
| **Native** | HTML and the browser: links, forms, `<details>`, `popover`        | None                  |
| **Server** | A mutation; the server answers with the changed parts of the page | Dispatcher only       |
| **Client** | A handler that changes client state                               | Dispatcher, one chunk |

### Native

Native elements work before any JS loads and keep working if it never loads:

```tsx
<details>
  <summary>Filters</summary>
  <FilterList />
</details>

<button popovertarget="menu">Menu</button>
<div id="menu" popover>
  …
</div>
```

The compiler emits these as plain HTML. They cost no client bytes.

The compiler NEVER lowers client state to native elements. A `signal` toggled by a button and
read only by a `hidden` binding stays in the Client tier, even though `popover` could replace it:
the native element brings its own focus, dismissal, and top-layer behavior, so the result would
depend on which pattern the compiler happened to recognize. Write the native element yourself
when you want it.

### Server

A handler whose body is one mutation call, with arguments the server knows at render time, needs
no client handler. The compiler lowers it to a form:

```tsx
export default function Todo(props: { todo: Todo }) {
  return <button onClick={() => toggleTodo({ id: props.todo.id })}>Toggle</button>;
}
```

```html
<form method="post" action="/todos?_m=k3x9f2"><input type="hidden" name="id" value="7" /><button>Toggle</button></form>
```

The arguments follow the hole rules of [Hole expressions](ssr.md#hole-expressions).

Without JS, the browser posts the form. The server runs the mutation and answers `303 See Other`
to the page that posted, so a reload NEVER resubmits it. Input that fails validation answers
`422` with the page rendered again, see [Report invalid input](ssr.md#report-invalid-input).

With the dispatcher, the post runs through `fetch`. The server answers with HTML, never with data
for bindings: applying data needs binding code, and binding code is the Client tier. The response
contains:

- the HTML of every route segment whose loaders reran, see
  [Revalidate after a mutation](ssr.md#revalidate-after-a-mutation);
- the state block for client handler sites inside those segments.

The client applies each segment with a morph, not a replacement. It walks the old and the new tree
together, updates text and attributes in place where the structure matches, and replaces a
subtree only where it differs. Focus, scroll position, and selection survive. The morph NEVER
touches state the user owns:

| Element                   | Kept from the live page |
| ------------------------- | ----------------------- |
| `<details>`, `<dialog>`   | `open`                  |
| `<input>`, `<textarea>`   | `value`, `checked`      |
| `<select>`                | `selected` options      |
| An element with `popover` | Shown or hidden         |

The morph is a separate module, not part of the dispatcher. The dispatcher loads it on intent over
a server-tier form, like a client chunk. Client navigation uses the same response and the same
morph, see [Client code](ssr.md#client-code).

### Client

A handler that writes client state runs in the browser:

```tsx
export function Menu(props: { items: Item[] }) {
  let open = signal(false);
  return (
    <nav>
      <button onClick={() => (open = !open)} aria-expanded={open}>
        Menu
      </button>
      <ul hidden={!open}>
        <For each={props.items}>
          {(item) => (
            <li>
              <a href={item.href}>{item.label}</a>
            </li>
          )}
        </For>
      </ul>
    </nav>
  );
}
```

The compiler reads the write and read graph:

- The handler writes `open`.
- `aria-expanded` and `hidden` read `open`. They are the bindings of this chunk.
- The list reads `props.items`, which never changes in the browser. It ships no code.
- `open` starts as the constant `false`, which the server rendered already. No state is sent.

The client chunk contains one handler and two attribute writes. The `<li>` elements, the links,
and the component function itself never reach the browser.

### Pending and optimistic UI

While a server-tier form posts, the dispatcher sets `aria-busy="true"` on it and ignores another
submit of the same form. CSS styles the wait, and no client code ships:

```css
form[aria-busy="true"] button {
  opacity: 0.5;
}
```

`useSubmission` is evaluated on the server and has no pending state.

Optimistic updates need no API. In the Client tier they are signals written before the call and
restored on failure:

```tsx
<button
  onClick={async () => {
    done = !done;
    try {
      await toggleTodo.call({ id: props.id });
    } catch {
      done = !done;
    }
  }}
>
  Toggle
</button>
```

The response morphs the server-rendered parts of the page. Signals stay client state.

## Loading on intent

### Dispatcher

The dispatcher is one external module, loaded with `async`, never render-blocking, and SHOULD
stay under 1 KB gzip. It:

1. Listens in the capture phase on `document`, only for the event types the page uses. The server
   writes that list into the page.
2. On an intent event over a handler site, starts `import()` of that site's chunk.
3. On the real event, runs the handler at once if the chunk is loaded. Otherwise it queues the
   event, awaits the chunk, and replays queued events in order.
4. When the browser is idle, preloads the chunks of handler sites in the viewport with
   `<link rel="modulepreload" fetchpriority="low">`. It skips this under `Save-Data` and on slow
   connections.

The time between `pointerdown` and `click` is often 50–150 ms, enough to fetch a small chunk from
a warm connection or a cache.

While a chunk loads, the dispatcher sets `aria-busy="true"` on the handler site and removes it
after the replay. The user sees that the action was accepted, and CSS can style the wait:

```css
[aria-busy="true"] {
  cursor: progress;
}
```

### Intent on touch devices

The intent events cover touch without `touchstart`:

| Event          | Mouse, pen                        | Touch                                     |
| -------------- | --------------------------------- | ----------------------------------------- |
| `pointerenter` | Hover, often hundreds of ms early | Fires right before `pointerdown`: ignored |
| `pointerdown`  | Press, 50–150 ms before `click`   | Contact, a tap's duration before `click`  |
| `focusin`      | Keyboard and programmatic focus   | Keyboard and programmatic focus           |

The dispatcher listens to `pointerenter` only when `pointerType` is `mouse` or `pen`. On touch,
`pointerdown` fires at contact, together with `touchstart`, and pages with
`<meta name="viewport" content="width=device-width">` have no tap delay. When the contact turns
into a scroll, the browser fires `pointercancel`; the started load continues at low priority and
costs only a request.

Under the default `"route"` granularity, the first contact on any handler site loads the whole
route chunk, and idle preloading covers the visible handler sites before any contact.

### Chunk granularity

Fewer chunks are the default. The compiler groups client code by route:

- Each route gets one chunk with the handlers and bindings of every handler site on that route.
- Code that appears on several routes goes into one shared chunk, so no handler is duplicated.

The first intent on a route loads its whole chunk, and every later interaction on that page runs
without a request. Idle preloading fetches the same chunk.

The granularity is a build option:

```ts file=vite.config.ts
reze({
  chunks: "route", // default
});
```

| Value         | One chunk per                       | Trade-off                                   |
| ------------- | ----------------------------------- | ------------------------------------------- |
| `"route"`     | Route, plus one shared chunk        | Fewest requests, more bytes on first intent |
| `"component"` | Component with client handler sites | Middle ground                               |
| `"handler"`   | Handler site                        | Fewest bytes per interaction, most requests |

### Failed chunk loads

A chunk fails to load for two reasons: the network dropped the request, or a deploy removed the
hashed file the page refers to. The dispatcher handles a failure in this order, with the queued
events kept until the last step:

1. **Retry once.** The retry adds a query to the URL (`c4.js?r=1`), because the browser MAY keep a
   failed module fetch in its module map and fail the same URL again.
2. **Fall back to the native action.** A link calls `location.assign(href)`. A form or a submit
   button calls `form.requestSubmit()`.
3. **Report.** The dispatcher drops the queued events and dispatches a cancelable `rz:chunkerror`
   event on `window`, with the handler site and the error in `detail`. Unless a listener calls
   `preventDefault()`, the dispatcher reloads the page, at most once per session: a flag in
   `sessionStorage` prevents a reload loop.

```ts
addEventListener("rz:chunkerror", (event) => {
  event.preventDefault();
  toast("Connection lost. Try again.");
});
```

Deploys MUST keep the assets of at least the previous build reachable. A page opened before a
deploy then keeps loading its chunks, and step 3 stays rare.

### Decide synchronously what cannot wait

Some decisions must be made inside the event, before any chunk can arrive. The compiler extracts
them from the handler into the markup, so the dispatcher applies them at once:

```tsx
<a
  href="/cart"
  onClick={(event) => {
    event.preventDefault();
    openCart();
  }}
>
  Cart
</a>
```

```html
<a href="/cart" data-rz="c2.0" data-rz-prevent="click">Cart</a>
```

Conditions on event fields that the compiler can express in markup stay lazy:

```tsx
<input
  onKeyDown={(event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      submit();
    }
  }}
/>
```

```html
<input data-rz="c2.1" data-rz-prevent="keydown:Enter" />
```

The supported conditions are `key`, `code`, `button`, and the modifier flags. Any other condition
on `preventDefault`, `stopPropagation`, or `stopImmediatePropagation` is an error:

```tsx
<a
  href="/next"
  onClick={(event) => {
    if (form.dirty) event.preventDefault();
  }}
>
  Next
</a>
// EVENT_DECISION_DEFERRED: `preventDefault` depends on `form.dirty`, which the dispatcher cannot
// evaluate before the chunk loads.
// Fix: move the check to a native mechanism (a `beforeunload` guard, `required`), or set
// `eager` on the element so its chunk loads before the first interaction.
```

### Limits of replay

- A replayed event has `isTrusted === false`. Code that checks it behaves differently.
- APIs that need transient user activation (`window.open`, clipboard writes, fullscreen, media
  with sound) work only while the activation from the original gesture lasts. A slow chunk can
  outlive it.
- `event.timeStamp` is the original event's time, not the replay time.

### Code that runs without an interaction

`effect`, timers, and observers run without a user action. The compiler loads their chunks when
the element enters the viewport, or when the browser is idle for elements that need it without
being visible.

## Module state

State declared at module level is shared by every component that imports it:

```ts file=src/cart.ts
export let count = signal(0);
```

- **One instance in the browser.** The module goes into a shared chunk, so every route reads the
  same `count`.
- **Read-only on the server.** A server process serves many users, so a write during the server
  render would leak between requests. The compiler reports it as `MODULE_STATE_SERVER_WRITE`. The
  initializer MUST be a constant: the server renders it, and the state block carries nothing.
  Values that come from the server are loader data, not module state.
- **Survives navigation.** JS modules live across client navigations, and so does their state.
  The HTML of a new segment shows the initial value, so after the morph the client runs the
  bindings that read written module state inside the new segments. The compiler knows these
  bindings; under `chunks: "route"`, their code is almost always loaded already.
- **Lost on reload.** Persisting it, for example in `localStorage`, is application code.

## HTML metadata

### Principle

The HTML carries only what the client cannot derive from the URL, the compiled template, or the
build manifest. Every byte of metadata competes with content for the first TCP window.

This is the kind of markup to avoid:

```html
<!--rz:l:%5B1%2C%22(main)%2Findex%22%2C%7B%7D%2C0%2C1%2C%22%22%2C%22%22%5D-->
<!--/rz:l:%5B1%2C%22(main)%2Findex%22%2C%7B%7D%2C0%2C1%2C%22%22%2C%22%22%5D--></div></div>
<!--/rz:l:%5B0%2C%22(main)%22%2C%7B%7D%2C0%2C0%2C%22%22%2C%22%22%5D--><!--/rz:0--></div>
<!--rz:0--><!--/rz:0--><!--rz:p--><!--/rz:p--><!--rz-shell-->
```

It repeats its payload in closing markers, URL-encodes JSON (each quote costs three bytes),
encodes a route id and params that the URL already determines, and emits ranges that are empty.

### Rules

1. **Static content has no markers.** A node no binding touches is plain HTML.
2. **Locate bindings by template path.** The compiler knows the template, so a binding's node is a
   fixed path from the nearest handler site or range: `firstChild`, `nextSibling`, and so on. A
   binding needs no attribute or comment of its own.
3. **Separate only adjacent dynamic text.** The client splits merged text by static lengths. A
   separator `<!>` is emitted only between two dynamic text nodes, see [Split text](#split-text).
4. **Mark ranges only when the node count varies.** A list or a condition whose content is not
   always one element gets one marker pair with no payload: `<!--[-->` … `<!--]-->`. Content that
   is always one element needs no markers.
5. **Closing markers carry no payload.** Never repeat an id or data in a closing marker.
6. **No empty ranges.** A slot that renders nothing and that no client code can fill emits
   nothing.
7. **Derive route state from the URL.** The client router computes the route id and params from
   the URL and its route table. The server NEVER writes them, nor `pathname`, `search`, or `hash`.
8. **Short identifiers.** Handler sites use per-page base-36 counters (`c2.0`). Modules are
   indexes into the build manifest, never source paths or export names.
9. **One state block per page.** All client state goes into one positional JSON block at the end of `<body>`,
   after the content, with no type tags and no field names.

The same handler site from the [Client](#client) example, rendered:

```html
<nav>
  <button aria-expanded="false" data-rz="c1.0">Menu</button>
  <ul hidden>
    <li><a href="/docs">Docs</a></li>
    <li><a href="/blog">Blog</a></li>
  </ul>
</nav>
```

One attribute. The dispatcher finds `aria-expanded` on the site itself and `hidden` at the path
`nextElementSibling`.

### Split text

The HTML parser merges adjacent text into one node, so `Hello, {name}!` arrives as one text node.
The compiler knows the static text, so the client splits the node by its length and needs no
separator:

```tsx
<p>Hello, {name}!</p>
```

```html
<p>Hello, Ada!</p>
```

The client cuts `"Hello, "` from the start and `"!"` from the end with `splitText`. The compiler
measures static text in UTF-16 code units, after decoding character references, the same units
`splitText` counts.

A dynamic value that renders as an empty string leaves no text node. The client locates the slot
by the neighboring element or by its parent and inserts an empty text node there.

Two adjacent dynamic texts have no static length between them. Only there the server emits a
separator:

```tsx
<p>
  {first}
  {last}
</p>
```

```html
<p>Ada<!>Lovelace</p>
```

`<!>` is 3 bytes against 7 for `<!---->`. It is a parse error by the specification, but HTML parse
errors are not fatal: the specification defines the recovery, and every browser produces the same
empty comment. Svelte and Solid use it in client templates that go through the same parser.
Validators report it; the framework accepts that.

### Measure

A test MUST report metadata bytes (markers, `data-rz*` attributes, the state block) as a share of
each page's compressed HTML on realistic applications. The share SHOULD stay under 2%.

## Diagnostics

| Code                        | Raised when                                                                       |
| --------------------------- | --------------------------------------------------------------------------------- |
| `MODULE_STATE_SERVER_WRITE` | The server render writes state declared at module level                           |
| `EVENT_DECISION_DEFERRED`   | `preventDefault` or `stopPropagation` depends on a condition markup can't express |
| `CLIENT_RANGE_UNSUPPORTED`  | A range client code updates contains content it cannot create                     |

## Implementation status

The current implementation differs from this design in these points:

- Under `chunks: "route"`, a route chunk imports the client code of each module it uses, and the
  bundler splits modules that several routes share into its own common chunks, so there can be
  more than one shared chunk.
- `chunks: "handler"` builds the same chunks as `"component"`: one per component with handlers.
  Every handler of a component shares that component's chunk.
- The Rust server takes the chunk mode from `reze_build::Config::chunks`, which must match the
  plugin option.
- The state block lists the factories that start without an event: components with an effect, a
  `ref`, or an `eager` element. Their chunk loads when one of their marked elements enters the
  viewport, or when the browser is idle if they have none. An `eager` element waits for the
  viewport like an effect.
- A store tracks each key of each object it wraps, and `Object.keys` and other enumerations
  of an object as one more field. A write notifies the readers of that key, a change of the key
  set notifies enumerations, and a change of an array's `length` notifies the readers of `length`
  and of the indexes it cut off.
- Every client range (a `Show`, `Switch`, `For`, or `Repeat` whose condition or source reads
  client state, or that sits inside one) is marked, even when its content is always one element.
  The start marker carries the scope and a mark, `<!--[0.3-->`, so client code finds it without
  a path. Rows that are not exactly one element open with `<!--,-->`.
- Client code creates the content of a client range from a template: elements, text, bindings,
  handlers, and nested ranges. Components, boundaries, `<head>`, `<Portal>`, `children`, spread
  attributes, computed form actions, and callback bodies with statements inside a client range
  raise `CLIENT_RANGE_UNSUPPORTED`.
- A prop is live when a call passes it an expression that reads client state or a live prop of
  the caller, or a function that the child's handlers, effects, or locals call. The parent
  instance gives each live prop as a getter for the call's site, and the child entry ends with
  `[distance, site]`, the link to the parent entry. A child with live props starts when its chunk
  loads, and in `component` and `handler` modes shares one chunk with the parents that link it.
  Calls inside a callback, `<Loading>`, `<Portal>`, or `dynamic(…)` do not link: the child reads
  the server values of its live props from its entry, and a function prop is `undefined` there.
  Spread props never become live.
- The state block writes a `Date` as milliseconds, a `Set` as an array, a `Map` as pairs, and a
  `bigint` as a decimal string; a `Record` stays an object. Client code converts a read back when
  the component's props annotation gives its type, or the loader result types `props.data` in a
  route component. A value whose type no annotation gives, such as a signal initialized from a
  call, reaches client code in its encoded form.
- Server-tier lowering covers only the `onClick` of a `<button>` without `type`, `form`, `name`,
  `value`, a ref, or spread props, outside a `<form>` and outside a range that reads client state.
  The handler takes no parameters, and its body is one call of a mutation, `toggleTodo(…)` or
  `toggleTodo.call(…)` since `Mutation` types only `.call`, with no argument or with
  one object literal whose fields are strings, numbers, or literal unions of them. Any other
  handler stays in the Client tier.
- A prevent condition is written as `type:cond+cond`. A condition is a bare `key` value, or
  `field=value` for `code`, `button`, and the modifier flags (`ctrlKey=true`). A key value that
  contains a space, `:`, `+`, or `=` cannot be written this way and raises
  `EVENT_DECISION_DEFERRED`.
- After a chunk fails to load, the dispatcher goes inert for the rest of the page. The native
  fallback, `rz:chunkerror` on `window`, or the single reload takes over.
- The dispatcher is about 1.9 KB gzip, above the 1 KB target: it also intercepts links,
  server-tier forms, and history navigation, and accepts the state blocks of navigations and
  streamed parts.
- `scripts/bench/size.mts` measures only the test applications, which are too small to judge the
  2% metadata share.
