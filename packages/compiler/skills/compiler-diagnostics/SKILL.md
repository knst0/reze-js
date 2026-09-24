# Repairing Reze compiler diagnostics

Generated from `crates/reze_compiler/src/diagnostic/catalog.rs`; do not edit by hand
(`REZE_UPDATE_SKILL=1 cargo test -p reze-compiler` regenerates it).

Every compiler diagnostic has a stable code, printed in brackets at the start of its message.
This guide maps each code to its repair.

## How to act on a diagnostic

1. If the diagnostic carries `fixes`, apply them: each fix is a list of exact source edits
   (`start`/`end` byte offsets, replacement `text`) that removes the diagnostic.
2. Otherwise follow the repair for its code below.
3. Do not suppress or work around a diagnostic you do not understand: every `error` is broken
   code and every `warn` is code that renders wrong or wastes work. `info` codes explain an
   optimization and need no action.

The `in` line (`path` in JSON) names the enclosing components and elements, root first:
`in <App> › ul › <For>`. Structured output for tooling: the Vite plugin option
`diagnostics: { jsonl: "path" }` appends one JSON object per diagnostic.

## PARSE_ERROR

**The file does not parse** · severity `error`

The parser rejected the source; nothing was compiled.

**Repair:** Fix the syntax at the reported position. The message is the parser's own. JSX-specific causes: an unclosed tag, `{` without `}`, or JSX in a `.ts` file (rename it to `.tsx`).

Before:

```tsx
const a = <div>;
```

After:

```tsx
const a = <div />;
```

## CLASS_ALIAS

**`className` / `classList` instead of `class`** · severity `warn`

A native element uses `className` or `classList`. Reze has one class attribute, `class`, which accepts a string, a toggle object, or a (nested) array of both. The compiler compiled the alias as `class` and merged it with any other class sources on the element.

**Repair:** Apply the fix: rename the attribute to `class`. When the element has several class sources, merge them into one array: `class={["btn", { active: on() }]}`.

Before:

```tsx
<button className="btn" classList={{ active: on() }} />
```

After:

```tsx
<button class={["btn", { active: on() }]} />
```

## CHILDREN_PROP_IGNORED

**`children` attribute next to nested children** · severity `warn`

An element has both a `children` attribute and nested JSX children. Nested children win; the attribute is never rendered.

**Repair:** Apply the fix to remove the attribute, or move its value between the tags.

Before:

```tsx
<div children={a()}>
  <b />
</div>
```

After:

```tsx
<div>
  {a()}
  <b />
</div>
```

## KEY_ON_ELEMENT

**`key` on a native element** · severity `warn`

`key` has no meaning in Reze: list rows are keyed by item identity in `<For>`. On a native element it renders as a useless `key` attribute.

**Repair:** Apply the fix to remove the attribute. To key rows, render the list with `<For each={items()}>`.

Before:

```tsx
{
  items().map((item) => <li key={item.id}>{item.name}</li>);
}
```

After:

```tsx
<For each={items()}>{(item) => <li>{item.name}</li>}</For>
```

## DUPLICATE_ATTRIBUTE

**The same attribute twice on one element** · severity `warn`

An element sets the same attribute more than once. The last one wins; the earlier ones are dead code.

**Repair:** Apply the fix to remove the earlier attribute (marked by the secondary label), or merge both values into one.

Before:

```tsx
<a href="/a" href={url()} />
```

After:

```tsx
<a href={url()} />
```

## UNKNOWN_ATTRIBUTE

**Probable attribute typo** · severity `warn`

The attribute is not a known HTML/SVG attribute but is one or two edits away from one. It renders exactly as written, so the browser ignores it.

**Repair:** Apply the fix to rename it to the suggestion. Custom attributes should use a `data-` prefix, which is never checked.

Before:

```tsx
<div clas="box" />
```

After:

```tsx
<div class="box" />
```

## EVENT_NAME_LOWERCASE

**Lower-case event attribute with a function** · severity `warn`

`onclick={fn}` passes a function to a lower-case `on…` attribute. Only camel-case `onClick` (or `on:click`) attaches a listener; the compiler compiled it as `onClick`.

**Repair:** Apply the fix: rename the attribute to camel case (`onClick`), or use `on:click` for a non-delegated listener.

Before:

```tsx
<button onclick={() => save()} />
```

After:

```tsx
<button onClick={() => save()} />
```

## SIGNAL_NOT_CALLED

**Signal passed without calling it** · severity `warn`

A signal or computed getter is passed as an attribute, property or style value without being called. The DOM receives the function itself (its source text as the attribute value), not the current value, and never updates.

**Repair:** Apply the fix: call the getter, `count()`. The compiler tracks the call and updates the attribute when the signal changes.

Before:

```tsx
<input value={count} title={label} />
```

After:

```tsx
<input value={count()} title={label()} />
```

## PROPS_DESTRUCTURED

**Component props destructured in the parameter list** · severity `warn`

A component destructures its props in the parameters. Props are getters: destructuring reads each one once, when the component runs, so the component never sees later updates.

**Repair:** Take `props` as one parameter and read `props.name` where the value is used (in JSX, a memo or an effect). Use `splitProps` to forward a subset.

Before:

```tsx
function Greeting({ name }) {
  return <p>Hello {name}</p>;
}
```

After:

```tsx
function Greeting(props) {
  return <p>Hello {props.name}</p>;
}
```

## INLINE_EACH

**Inline array literal in `<For each>`** · severity `warn`

`<For each={[…]}>` creates a new array with new identity on every evaluation, so every row is rebuilt each time the parent re-renders.

**Repair:** Hoist the array to a module constant, or hold it in a signal or memo.

Before:

```tsx
<For each={[1, 2, 3]}>{(n) => <li>{n}</li>}</For>
```

After:

```tsx
const numbers = [1, 2, 3];
<For each={numbers}>{(n) => <li>{n}</li>}</For>;
```

## ASYNC_COMPONENT_SHAPE

**Async component outside the supported shape** · severity `warn`

An `async` function that renders JSX was left as written: its body is not `const x = await …;` declarations followed by `return …;`. `data.reason` names the failed rule. Left as written, it returns a Promise, which renders nothing.

**Repair:** Reshape the body: top-level `const x = await fetchX(…);` declarations (one declarator each), other statements between them allowed, no `return`/`throw` before the last `await`, a final `return <…/>;`, and no local from before an `await` used after it.

Before:

```tsx
async function User(props) {
  const label = props.label;
  const user = await fetchUser(props.id);
  return (
    <p>
      {label}: {user.name}
    </p>
  );
}
```

After:

```tsx
async function User(props) {
  const user = await fetchUser(props.id);
  return (
    <p>
      {props.label}: {user.name}
    </p>
  );
}
```

## ASYNC_RETURN_TYPE

**Async component return type cannot be unwrapped** · severity `warn`

The compiled component is synchronous, so its `Promise<T>` annotation is unwrapped to `T`. This annotation names `Promise` in a form the compiler cannot unwrap and was kept, so the emitted TypeScript is wrong.

**Repair:** Annotate as `Promise<JSX.Element>` or remove the return type annotation.

Before:

```tsx
async function User(): Promise { … }
```

After:

```tsx
async function User(): Promise<JSX.Element> { … }
```

## SIGNAL_FOLDED

**Constant signal folded** · severity `info`

The signal's setter is never used and its getter is only ever called, so the compiler replaced it with a plain constant (optimization O3). Reads cost nothing and literal values render straight into the template.

**Repair:** Nothing to repair. If the value is meant to change, call its setter somewhere; the fold disappears on its own.

Example:

```tsx
const [title] = signal("Reze");
<h1>{title()}</h1>;
```

## DEAD_BRANCH_REMOVED

**Dead JSX branch removed** · severity `info`

A child's condition is a literal, so one branch can never render. The compiler dropped it together with its templates and runtime imports (optimization O5).

**Repair:** Nothing to repair. Delete the dead branch from the source to make the intent explicit.

Example:

```tsx
{
  DEBUG && <DebugPanel />;
}
```
