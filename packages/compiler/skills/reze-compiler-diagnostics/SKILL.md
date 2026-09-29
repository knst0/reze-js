---
name: reze-compiler-diagnostics
description: Repair guide for reze-js compiler diagnostics. Use when a build, dev server or test prints a bracketed reze code such as [UNKNOWN_ATTRIBUTE], or when diagnostics.jsonl contains reze diagnostics.
---

# reze-js compiler diagnostics

<!-- Generated from crates/reze_compiler/src/diagnostic/catalog.rs. Do not edit by hand: regenerate with `REZE_UPDATE_SKILL=1 cargo test -p reze_compiler --test catalog`. -->

Every diagnostic starts with its code in brackets, `[CODE]`, and says what was seen, why it is wrong and what to do.

## How to act

1. When the diagnostic has `fixes`, apply them: each fix is a list of exact byte edits (`start`, `end`, `text`) into the source that removes the diagnostic.
2. Otherwise follow the **Repair** of the code below.
3. Never suppress, silence or work around an `error` or a `warn`; fix its cause.
4. An `info` explains what the compiler did; it needs no action.

## Reading the `in` path

The `in` line lists what encloses the reported code, outermost first: components as `<Name>` and native elements by tag, for example `in <App> › <TodoList> › li › button`. Start at the last entry.

## JSONL channel

With the Vite plugin option `reze({ diagnostics: { jsonl: "path" } })`, every diagnostic of every severity is appended to `path` as one JSON object per line, with `code`, `severity`, `message`, `file`, `start`/`end` (`offset`, 1-based `line`, 0-based UTF-16 `column`), `path`, `labels`, `fixes`, `data`, `docs` and `rendered`.

## Codes

## PARSE_ERROR

**The file does not parse** · severity `error`

> The parser rejected this file ({detail}), so nothing was compiled. Fix the syntax at this position.

`data` keys: `detail`

Automatic fix: no

The parser reported a syntax error. `data.detail` is the parser's own message. Nothing of the file was compiled.

**Repair:** Fix the syntax at the reported position. JSX-specific causes: an unclosed tag, `{` without `}`, or JSX in a `.ts` file (rename it to `.tsx`).

Before:

```tsx
export const view = <div>;
```

After:

```tsx
export const view = <div />;
```

## CONTROL_FLOW_AS_VALUE

**Control-flow tag used as a value** · severity `error`

> `{name}` is compiled away and has no runtime value, so this reference would throw. Use `{name}` only as a JSX tag.

`data` keys: `name`

Automatic fix: no

`Show`, `For`, `Repeat`, `Switch`, `Match`, `Loading` and `Errored` are compiler intrinsics: every `<Show>` tag compiles to direct runtime calls, and the imported function only throws. Passing the import around, calling it, or re-exporting it would reach that function at runtime. `data.name` is the intrinsic.

**Repair:** Render it as a tag, `<Show when={…}>…</Show>`. To pick a component at runtime, wrap the tag in a component of your own and pass that.

Before:

```tsx
import { Show } from "reze-js";

export const Conditional = Show;
```

After:

```tsx
import { Show } from "reze-js";

export const view = <Show when={open()}><p>open</p></Show>;
```

## CONTROL_FLOW_ATTRIBUTE

**Attribute a control-flow tag does not accept** · severity `error`

> `<{tag}>` does not accept `{attribute}`, so it would be silently dropped. Remove it.

`data` keys: `attribute`, `tag`

Automatic fix: yes

A control-flow tag accepts a fixed set of attributes: `<Show when fallback>`, `<For each fallback key>`, `<Repeat count fallback>`, `<Switch fallback>`, `<Match when>`, `<Loading fallback>`, `<Errored fallback>`. Anything else, spreads included, has no meaning. `data.tag` is the tag and `data.attribute` the attribute (`{...}` for a spread).

**Repair:** Apply the fix to remove the attribute. To key `<For>` rows by a field, use `key={(item) => item.id}`.

Before:

```tsx
import { Show } from "reze-js";

export const view = <Show when={open()} keyed><p>open</p></Show>;
```

After:

```tsx
import { Show } from "reze-js";

export const view = <Show when={open()}><p>open</p></Show>;
```

## CONTROL_FLOW_MISSING

**Control-flow tag without its required attribute** · severity `error`

> `<{tag}>` needs `{attribute}` to decide what to render. Add `{attribute}={…}`.

`data` keys: `attribute`, `tag`

Automatic fix: no

`<Show>` and `<Match>` render by `when`, `<For>` by `each`, `<Repeat>` by `count`. Without it there is nothing to decide on. `data.tag` is the tag and `data.attribute` the missing attribute.

**Repair:** Add the attribute with the condition (`when`), the list (`each`) or the number of rows (`count`).

Before:

```tsx
import { Show } from "reze-js";

export const view = <Show fallback={<p>closed</p>}><p>open</p></Show>;
```

After:

```tsx
import { Show } from "reze-js";

export const view = <Show when={open()} fallback={<p>closed</p>}><p>open</p></Show>;
```

## CONTROL_FLOW_CHILDREN

**Control-flow tag with children it cannot render** · severity `error`

> `<{tag}>` expects {expected} as children, so this cannot be compiled.

`data` keys: `expected`, `tag`

Automatic fix: no

`<Show>`, `<Match>`, `<Loading>` and `<Errored>` need at least one child. `<For>` needs exactly one row function `(item, index) => …` and `<Repeat>` one `(index) => …`, as its child or its `children` attribute. `<Switch>` only takes `<Match>` elements. `data.tag` is the tag and `data.expected` what it takes.

**Repair:** Give the tag the children it expects: wrap `<For>` rows in `{(item) => …}`, move non-`<Match>` children of `<Switch>` into a `<Match>` or its `fallback`.

Before:

```tsx
import { For } from "reze-js";

export const view = <ul><For each={items()}><li>item</li></For></ul>;
```

After:

```tsx
import { For } from "reze-js";

export const view = <ul><For each={items()}>{(item) => <li>{item()}</li>}</For></ul>;
```

## MATCH_OUTSIDE_SWITCH

**`<Match>` outside `<Switch>`** · severity `error`

> `<Match>` only works as a direct child of `<Switch>`. Move it into a `<Switch>`.

`data` keys: none

Automatic fix: no

`<Match>` is a case of the `<Switch>` around it; on its own, or nested deeper, there is no switch to choose it.

**Repair:** Make the `<Match>` a direct child of a `<Switch>`, or use `<Show when={…}>` for a single condition.

Before:

```tsx
import { Match } from "reze-js";

export const view = <div><Match when={ready()}><b>ready</b></Match></div>;
```

After:

```tsx
import { Match, Switch } from "reze-js";

export const view = <div><Switch><Match when={ready()}><b>ready</b></Match></Switch></div>;
```

## CHILDREN_PROP_IGNORED

**`children` attribute next to nested children** · severity `warn`

> `children` is set both as an attribute and as nested children, and nested children win, so the attribute is never rendered. Remove the attribute.

`data` keys: none

Automatic fix: yes

An element or component has both a `children` attribute and nested children. The nested children are rendered; the attribute is dead code.

**Repair:** Apply the fix to remove the attribute, or move its value between the tags.

Before:

```tsx
export const view = <div children={label()}><b /></div>;
```

After:

```tsx
export const view = <div>{label()}<b /></div>;
```

## KEY_ON_ELEMENT

**`key` on a native element** · severity `warn`

> `key` does nothing on a native element and renders as a useless attribute, since list rows are keyed by `<For>`. Remove it.

`data` keys: none

Automatic fix: yes

Reze has no virtual DOM to reconcile by `key`: `<For>` keys its rows by item identity, or by its own `key` function. On a native element `key` is an ordinary attribute.

**Repair:** Apply the fix to remove the attribute. To key rows, render the list with `<For each={items()} key={(item) => item.id}>`.

Before:

```tsx
export const view = <li key="a">a</li>;
```

After:

```tsx
export const view = <li>a</li>;
```

## DUPLICATE_ATTRIBUTE

**The same attribute twice on one element** · severity `warn`

> `{attribute}` is set twice on this element and the last one wins, so the earlier one is dead code. Remove it or merge the values.

`data` keys: `attribute`

Automatic fix: yes

An element sets the same attribute more than once. Only the last value is used; the label marks the overridden one. `data.attribute` is the name.

**Repair:** Apply the fix to remove the earlier attribute, or merge both values into one.

Before:

```tsx
export const view = <a href="/a" href={url()} />;
```

After:

```tsx
export const view = <a href={url()} />;
```

## UNKNOWN_ATTRIBUTE

**Probable attribute typo** · severity `warn`

> `{attribute}` is not a known attribute and renders as written, so the browser ignores it. Did you mean `{suggestion}`?

`data` keys: `attribute`, `suggestion`

Automatic fix: yes

The attribute is not a known HTML or SVG attribute but is one or two edits away from one, or it is React's `className`/`classList`. It renders exactly as written. `data.attribute` is the name and `data.suggestion` the known one.

**Repair:** Apply the fix to rename it to the suggestion. Custom attributes take a `data-` prefix, which is never checked.

Before:

```tsx
export const view = <div clas="box" />;
```

After:

```tsx
export const view = <div class="box" />;
```

## EVENT_NAME_LOWERCASE

**Lower-case event attribute with a function** · severity `warn`

> `{attribute}` passes a function to a lower-case attribute, which never attaches a listener. Rename it to `{suggestion}`, which is how it was compiled.

`data` keys: `attribute`, `suggestion`

Automatic fix: yes

Only camel-case `onClick` (or `on:click`) attaches a listener; a lower-case `onclick` is an HTML attribute that takes code as a string. The compiler compiled it as the camel-case name. `data.attribute` is the name and `data.suggestion` the camel-case one.

**Repair:** Apply the fix to rename the attribute to camel case, or use `on:click` for a listener that is not delegated.

Before:

```tsx
export const view = <button onclick={() => save()} />;
```

After:

```tsx
export const view = <button onClick={() => save()} />;
```

## SIGNAL_NOT_CALLED

**Signal passed without calling it** · severity `warn`

> `{signal}` is a signal getter passed without calling it, so the DOM receives the function and never updates. Call it: `{signal}()`.

`data` keys: `signal`

Automatic fix: yes

A `signal` or `computed` getter is an attribute, property or style value without being called. The DOM receives the function itself, not the current value. Children and event handlers take functions legitimately and are not checked. `data.signal` is the getter.

**Repair:** Apply the fix to call the getter. The call is tracked, so the attribute follows the signal.

Before:

```tsx
import { signal } from "reze-js";

const [count, setCount] = signal(0);
export const view = <input value={count} onInput={() => setCount(1)} />;
```

After:

```tsx
import { signal } from "reze-js";

const [count, setCount] = signal(0);
export const view = <input value={count()} onInput={() => setCount(1)} />;
```

## PROPS_DESTRUCTURED

**Props destructured in a form that cannot be rewritten** · severity `warn`

> `{component}` destructures its props in a form the compiler cannot rewrite ({reason}), so each value is read once and never updates. Take `props` and read `props.x` where it is used.

`data` keys: `component`, `reason`

Automatic fix: no

Destructured props are rewritten into lazy reads of one props object, unless the pattern uses a computed key (`computed-key`), a default that cannot run once at the start (`default`), a default on a nested pattern (`nested-default`), a nested rest (`nested-rest`), an array pattern (`array-pattern`), a name that is assigned (`written`), `arguments` (`arguments`), a generator (`generator`), or more than one parameter (`params`). `data.component` is the component and `data.reason` the rule.

**Repair:** Take `props` as the one parameter and read `props.name` where the value is used. Use `splitProps` to forward a subset.

Before:

```tsx
export function Greeting({ name = fallback() }) {
  return <p>Hello {name}</p>;
}
```

After:

```tsx
export function Greeting(props) {
  return <p>Hello {props.name ?? fallback()}</p>;
}
```

## INLINE_EACH

**Inline array literal in `<For each>`** · severity `warn`

> `<For each={[…]}>` builds a new array on every evaluation, so every row is rebuilt each time. Hoist the array to a constant or keep it in a signal.

`data` keys: none

Automatic fix: no

An array literal has new identity, and new items, each time `each` is evaluated, so `<For>` cannot reuse any row.

**Repair:** Hoist the array to a module constant, or hold it in a signal or computed.

Before:

```tsx
import { For } from "reze-js";

export const view = <ul><For each={[1, 2, 3]}>{(n) => <li>{n()}</li>}</For></ul>;
```

After:

```tsx
import { For } from "reze-js";

const numbers = [1, 2, 3];
export const view = <ul><For each={numbers}>{(n) => <li>{n()}</li>}</For></ul>;
```

## ASYNC_COMPONENT_SHAPE

**Async component in a form that cannot be compiled** · severity `warn`

> `{component}` is an async component the compiler cannot rewrite ({reason}), so it stays an `async` function that returns a Promise and renders nothing. Reshape the awaits or move the work into an `asyncComputed`.

`data` keys: `component`, `reason`

Automatic fix: no

An `async` component compiles to a load step, re-run when a source it reads changes, and a body that runs once after the first load and reads each awaited value through a getter, so later loads update it in place. That needs each top-level `await` to be a whole statement, `const x = await …;` or `await …;` (`await-position`, and `nested-await` when its operand awaits again); between the first and last await only `const`/`let`/`var` declarations may appear (`statement-between-awaits`); nothing up to the last await may `return` (`return-before-await`) or contain JSX (`jsx-before-await`); no value an await produces may be assigned after the last await (`value-reassigned`). `data.component` is the component and `data.reason` the rule.

**Repair:** Give every await its own `const x = await …;` statement, keep other statements before the first await or after the last one, and start the JSX after the last await.

Before:

```tsx
export async function Card(props) {
  const user = await fetchUser(props.id);
  log(user);
  const posts = await fetchPosts(user.id);
  return <p>{posts.length}</p>;
}
```

After:

```tsx
export async function Card(props) {
  const user = await fetchUser(props.id);
  const posts = await fetchPosts(user.id);
  log(user);
  return <p>{posts.length}</p>;
}
```

## SIGNAL_FOLDED

**Constant signal folded** · severity `info`

> `{signal}` is never written and every read is a call, so it compiled to a plain constant.

`data` keys: `signal`

Automatic fix: no

The signal's setter is unused, its getter is only called, and neither is exported, so the signal became a constant: reads cost nothing and literal values render straight into the template. `data.signal` is the getter.

**Repair:** Nothing to repair. Call the setter somewhere and the fold disappears.

Example:

```tsx
import { signal } from "reze-js";

const [title] = signal("Reze");
export const view = <h1>{title()}</h1>;
```

## DEAD_BRANCH_REMOVED

**Dead JSX branch removed** · severity `info`

> The condition is a literal, so this branch can never render; it was removed.

`data` keys: none

Automatic fix: no

A child's condition is a literal, so one branch never renders. It was dropped with its templates and runtime imports.

**Repair:** Nothing to repair. Delete the dead branch to make the intent explicit.

Example:

```tsx
export const view = <div>{false && <b>debug</b>}</div>;
```

## PROPS_REWRITTEN

**Destructured props rewritten to lazy reads** · severity `info`

> `{component}` destructures its props, so the pattern became one props object and each destructured name a lazy read of it.

`data` keys: `component`

Automatic fix: no

Each destructured name reads the props object where it is used, so it stays reactive. Defaults apply when the prop is `undefined`; a rest element becomes `splitProps`. `data.component` is the component.

**Repair:** Nothing to repair.

Example:

```tsx
export function Greeting({ name }) {
  return <p>Hello {name}</p>;
}
```

## AUTO_SELECTOR

**Row comparison compiled to a selector** · severity `info`

> Each row compares `{signal}()` with its own key, so a change would re-run every row; the comparison now reads a selector created once for this `<For>`.

`data` keys: `signal`

Automatic fix: no

A `<For>` row compares a `signal`/`computed` declared outside the row with a key built from the row's parameters. One `selector` per `<For>` replaces the comparison, so a change re-runs only the rows whose result flips. `data.signal` is the getter.

**Repair:** Nothing to repair. Compare inside a nested function, or with something that is not the row's key, and it stays a plain comparison.

Example:

```tsx
import { For, signal } from "reze-js";

const [selected, setSelected] = signal(0);
export const view = (
  <ul>
    <For each={rows()}>
      {(row) => <li class={selected() === row().id ? "on" : ""} onClick={() => setSelected(row().id)} />}
    </For>
  </ul>
);
```
