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

## PARSER_RELOCATION

**Markup the HTML parser moves or rewrites** · severity `error`

> The browser parses this markup differently ({detail}), so the template would not match the rendered DOM. {hint}.

`data` keys: `detail`, `hint`

Automatic fix: no

Templates pass through the HTML parser before bindings run. Table rows and columns may acquire implied containers; stray table text is foster-parented out; raw text ends at its matching end tag; foreign elements retain their parser-selected namespace. `data.detail` identifies the mismatch and `data.hint` describes a representable structure.

**Repair:** {hint}.

Before:

```tsx
export const view = <table>oops<tr><td>cell</td></tr></table>;
```

After:

```tsx
export const view = <table><tbody><tr><td>cell</td></tr></tbody></table>;
```

## CONTROL_FLOW_AS_VALUE

**Control-flow tag used as a value** · severity `error`

> `{name}` is compiled away and has no runtime value, so this reference would throw. Use `{name}` only as a JSX tag.

`data` keys: `name`

Automatic fix: no

`Show`, `For`, `Repeat`, `Switch`, `Match`, `Loading`, `Errored` and `Portal` are compiler intrinsics: every `<Show>` tag compiles to direct runtime calls, and the imported function only throws. Passing the import around, calling it, or re-exporting it would reach that function at runtime. `data.name` is the intrinsic.

**Repair:** Render it as a tag, `<Show when={…}>…</Show>`. To pick a component at runtime, wrap the tag in a component of your own and select that with `dynamic(() => …)`.

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

A control-flow tag accepts a fixed set of attributes: `<Show when fallback>`, `<For each fallback keyed>`, `<Repeat count fallback>`, `<Switch fallback>`, `<Match when>`, `<Loading fallback>`, `<Errored fallback>`, `<Portal mount>`. Anything else, spreads included, has no meaning. `data.tag` is the tag and `data.attribute` the attribute (`{...}` for a spread).

**Repair:** Apply the fix to remove the attribute. To key `<For>` rows by a field, use `keyed={(item) => item.id}`.

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

`<Show>`, `<Match>`, `<Loading>`, `<Errored>` and `<Portal>` need at least one child. `<For>` needs exactly one row function `(item, index) => …` and `<Repeat>` one `(index) => …`, as its child or its `children` attribute. `<Switch>` only takes `<Match>` elements. `data.tag` is the tag and `data.expected` what it takes.

**Repair:** Give the tag the children it expects: wrap `<For>` rows in `{(item) => …}`, move non-`<Match>` children of `<Switch>` into a `<Match>` or its `fallback`.

Before:

```tsx
import { For } from "reze-js";

export const view = <ul><For each={items()}><li>item</li></For></ul>;
```

After:

```tsx
import { For } from "reze-js";

export const view = <ul><For each={items()}>{(item) => <li>{item}</li>}</For></ul>;
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

## SIGNAL_NOT_DECLARED

**`signal` or `computed` outside a variable declaration** · severity `error`

> `{primitive}` is only valid as the initializer of `let name = {primitive}(…)` or `const name = {primitive}(…)`. The compiler rewrites every use of that name, so it needs the declaration to see it.

`data` keys: `primitive`

Automatic fix: no

`signal(…)` and `computed(…)` are compiler syntax, not functions: the declared variable becomes a getter (and, for a written `signal`, a setter), and each read and write of it is rewritten. A call anywhere else, a `var` declaration, the syntax passed around as a value or re-exported has no variable to rewrite. `data.primitive` is the syntax used.

**Repair:** Declare the variable with `let name = signal(…)` or `const name = computed(…)` and use `name` where the value is needed. To pass the current value around, read `name` where it is needed or wrap the read in a function: `() => name`.

Before:

```tsx
import { signal } from "reze-js";

export const view = <p>{signal(0)}</p>;
```

After:

```tsx
import { signal } from "reze-js";

let count = signal(0);
export const view = <button onClick={() => (count += 1)}>{count}</button>;
```

## SIGNAL_PATTERN

**`signal` or `computed` destructured** · severity `error`

> `{primitive}(…)` initializes a destructuring pattern. A reactive variable is one name: declare `const name = {primitive}(…)`.

`data` keys: `name`, `primitive`

Automatic fix: yes

The declared name is rewritten into a getter (and, for a written `signal`, a setter), which needs exactly one identifier to rename. `data.primitive` is the syntax used.

**Repair:** Apply the fix for the `[get, set]` tuple of other libraries: it declares one name, turns `name()` reads into `name` and `setName(value)` statements into `name = value`. Without a fix, declare one name per `signal` or `computed`, for example `let count = signal(0)`, and assign to it to write it.

Before:

```tsx
import { signal } from "reze-js";

const [count, setCount] = signal(0);
export const view = <button onClick={() => setCount(count() + 1)}>{count()}</button>;
```

After:

```tsx
import { signal } from "reze-js";

let count = signal(0);
export const view = <button onClick={() => (count += 1)}>{count}</button>;
```

## SIGNAL_EXPORTED

**`signal` or `computed` exported** · severity `error`

> `{signal}` is a `{primitive}` and cannot be exported: an importing module cannot know it is reactive, so it would read a getter function or lose updates.

`data` keys: `primitive`, `signal`

Automatic fix: no

`signal` and `computed` variables exist only in the file that declares them; the compiler rewrites their uses there and nowhere else. `data.signal` is the exported variable and `data.primitive` the syntax it was declared with.

**Repair:** Keep the variable private and export a function that reads it, or hold the shared state in a `store`.

Before:

```tsx
import { signal } from "reze-js";

export let count = signal(0);
```

After:

```tsx
import { signal } from "reze-js";

let count = signal(0);
export const readCount = () => count;
```

## SIGNAL_ASSIGN_PATTERN

**`signal` written through a pattern** · severity `error`

> `{signal}` is written through a destructuring pattern or a `for` loop head, which the compiler cannot turn into a setter call. Assign with `{signal} = value`.

`data` keys: `signal`

Automatic fix: no

A write to a `signal` becomes a setter call. In `[a] = list`, `({ a } = obj)` and `for (a of list)` the assignment is done by the language, not by an expression the compiler can replace. `data.signal` is the written variable.

**Repair:** Read the value into a temporary and assign it with `name = value` in a statement.

Before:

```tsx
import { signal } from "reze-js";

let count = signal(0);
export function pick(list) {
  [count] = list;
}
export const view = <p>{count}</p>;
```

After:

```tsx
import { signal } from "reze-js";

let count = signal(0);
export function pick(list) {
  count = list[0];
}
export const view = <p>{count}</p>;
```

## SIGNAL_UPDATE_IN_EXPRESSION

**`++`/`--` on a `signal` inside an expression** · severity `error`

> `{signal}` is incremented or decremented as part of an expression. Use it as a statement, or write `{signal} += 1`, which is an expression with the new value.

`data` keys: `signal`

Automatic fix: no

`count++` as a statement becomes `setCount(count() + 1)`. Inside a larger expression, for example `() => count++` or `use(count++)`, the value of the old `++` would change meaning, so it is not accepted. `data.signal` is the updated variable.

**Repair:** Put the update in its own statement, or use `count += 1` (value: the new count) or `count -= 1`.

Before:

```tsx
import { signal } from "reze-js";

let count = signal(0);
export const view = <button onClick={() => count++}>{count}</button>;
```

After:

```tsx
import { signal } from "reze-js";

let count = signal(0);
export const view = <button onClick={() => { count++; }}>{count}</button>;
```

## COMPUTED_WRITTEN

**`computed` written** · severity `error`

> `{computed}` is a `computed` and cannot be written: its value is derived from what its expression reads. Write to those signals instead.

`data` keys: `computed`

Automatic fix: no

A `computed` variable is a derived getter with no setter. Assignments, compound assignments, `++`/`--` and writes through a pattern or a `for` loop head are refused. `data.computed` is the written variable.

**Repair:** Write the `signal` the expression reads, or declare the variable with `signal` when it is meant to be set directly.

Before:

```tsx
import { computed, signal } from "reze-js";

let count = signal(0);
const doubled = computed(count * 2);
export const view = <button onClick={() => (doubled = 0)}>{doubled}</button>;
```

After:

```tsx
import { computed, signal } from "reze-js";

let count = signal(0);
const doubled = computed(count * 2);
export const view = <button onClick={() => (count = 0)}>{doubled}</button>;
```

## COMPUTED_FUNCTION

**Function literal passed to `computed`** · severity `error`

> `computed` takes the expression itself and wraps it in a function, so a function literal here would make the derived value that function. Write `computed(expression)` without `() =>`.

`data` keys: none

Automatic fix: yes

`const doubled = computed(count * 2)` derives `doubled` from `count * 2`. A function literal as the argument is refused rather than quietly deriving a function. A derived value that is a function comes from another expression: a call, a conditional or an identifier.

**Repair:** Apply the fix to remove `() =>`. For several statements, move them into a function and derive its call: `computed(compute())`.

Before:

```tsx
import { computed, signal } from "reze-js";

let count = signal(0);
const doubled = computed(() => count * 2);
export const view = <button onClick={() => (count += 1)}>{doubled}</button>;
```

After:

```tsx
import { computed, signal } from "reze-js";

let count = signal(0);
const doubled = computed(count * 2);
export const view = <button onClick={() => (count += 1)}>{doubled}</button>;
```

## COMPUTED_AWAIT

**`await` or `yield` in `computed`** · severity `error`

> The expression of `computed` awaits or yields, but it is derived synchronously, where `await` and `yield` are not valid. Await the value in an async component and derive from the result.

`data` keys: none

Automatic fix: no

`computed(expression)` derives its value synchronously. An `await` or `yield` outside a nested function cannot move into it.

**Repair:** Await the value in an async component before the derivation, then derive from the loaded value.

Before:

```tsx
import { computed, signal } from "reze-js";

let id = signal(1);
const label = computed(await describe(id));
export const view = <p onClick={() => (id += 1)}>{label}</p>;
```

After:

```tsx
import { computed } from "reze-js";

export async function Label(props) {
  const text = await describe(props.id);
  const label = computed(text.toUpperCase());
  return <p>{label}</p>;
}
```

## ACTION_ARGUMENT

**`action` without a function literal** · severity `error`

> `action` needs its body written at the call, as an arrow function or function expression, so the compiler can keep the action current across each `await`. Write `action(async (…) => { … })`.

`data` keys: none

Automatic fix: no

`action(fn)` rewrites `fn`: every `await` in its body resumes the action and the body ends it. An identifier, a call or a missing argument hides the body from the compiler.

**Repair:** Write the body in the call and call the existing function from it: `action(async (todo) => { await save(todo); })`.

Before:

```tsx
import { action } from "reze-js";

export const save = action(saveTodo);
```

After:

```tsx
import { action } from "reze-js";

export const save = action(async (todo) => {
  await saveTodo(todo);
});
```

## ACTION_UNSUPPORTED

**`for await`, `await using` or a generator in `action`** · severity `error`

> `{construct}` suspends the `action` body where the compiler cannot resume the action, so writes after it would escape the action. Use a plain `await` instead.

`data` keys: `construct`

Automatic fix: no

The compiler resumes the action after each `await` expression. `for await` and `await using` suspend without one, and a generator suspends at every `yield`. `data.construct` is what was found: `for await`, `await using` or `generator`.

**Repair:** Loop with `for (…) { const item = await next(); … }`, dispose with `try { … } finally { await resource[Symbol.asyncDispose](); }`, and pass an `async` function instead of a generator.

Before:

```tsx
import { action } from "reze-js";

export const load = action(async (list) => {
  for await (const item of stream()) list.push(item);
});
```

After:

```tsx
import { action } from "reze-js";

export const load = action(async (list) => {
  for (const item of await fetchAll()) list.push(item);
});
```

## ACTION_NOT_CALLED

**`action` used as a value** · severity `error`

> `action` is compiler syntax and has no runtime value, so this reference would fail. Call it with the body: `action(async (…) => { … })`.

`data` keys: none

Automatic fix: no

Only a call `action(fn)` is rewritten. Passing `action` around, storing it or re-exporting it leaves a reference to a function that does not exist at runtime.

**Repair:** Call `action` where the action is defined.

Before:

```tsx
import { action } from "reze-js";

export const make = action;
```

After:

```tsx
import { action } from "reze-js";

export const save = action(async (todo) => {
  await saveTodo(todo);
});
```

## PROPS_AS_VALUE

**`mergeProps`, `splitProps` or `omitProps` used as a value** · severity `error`

> `{name}` is compiler syntax and has no runtime value, so this reference would fail. Call it: `mergeProps(…)`, `splitProps(…)` or `omitProps(…)`.

`data` keys: `name`

Automatic fix: no

Only calls of the three props helpers are rewritten: dissolvable calls become object literals, the rest compiles to direct runtime calls. Passing a helper around, storing it or re-exporting it leaves a reference to a function that does not exist at runtime. `data.name` is the helper.

**Repair:** Call the helper where the props are handled.

Before:

```tsx
import { omitProps } from "reze-js";

export const view = omitProps;
```

After:

```tsx
import { omitProps } from "reze-js";

export const view = omitProps(props, "id");
```

## ISLAND_TRIGGER

**`island` with an unknown trigger** · severity `error`

> `{value}` is not an island trigger: `island` takes `eager`, `idle`, `visible`, `media` or `interaction`. Write `island` for the default, which is `eager`, or `island="visible"`.

`data` keys: `value`

Automatic fix: no

`island` marks a component as an island: it renders its fallback and loads the component when the trigger fires. The trigger is read at compile time, so it must be a bare attribute or a string literal. `data.value` is what was written.

**Repair:** Write `island` or one of the five triggers as a string literal. A trigger chosen at runtime is not supported.

Before:

```tsx
import { Counter } from "./Counter";

export const view = <Counter island="sometimes" />;
```

After:

```tsx
import { Counter } from "./Counter";

export const view = <Counter island="visible" />;
```

## ISLAND_MEDIA_MISSING

**`island="media"` without `islandMedia`** · severity `error`

> `island="media"` loads when a media query matches, so it needs `islandMedia="(…)"` with the query. Add it.

`data` keys: none

Automatic fix: no

A `media` island never observes an element: it loads when the query matches, immediately when it already does. Without the query there is nothing to match.

**Repair:** Add `islandMedia="(max-width: 40rem)"` next to `island="media"`.

Before:

```tsx
import { Counter } from "./Counter";

export const view = <Counter island="media" />;
```

After:

```tsx
import { Counter } from "./Counter";

export const view = <Counter island="media" islandMedia="(max-width: 40rem)" />;
```

## ISLAND_ON_ELEMENT

**`island` on a native element** · severity `error`

> `island` marks a component, but this is a native element, so there is no component to defer. Move `island` to a component.

`data` keys: none

Automatic fix: no

Only a component tag takes `island` and its companions `islandMedia`, `islandRootMargin` and `islandFallback`. On a native element they would render as useless attributes.

**Repair:** Move the attributes to the component that should load later.

Before:

```tsx
export const view = <section island="visible" />;
```

After:

```tsx
import { Counter } from "./Counter";

export const view = <Counter island="visible" />;
```

## ISLAND_ORPHAN

**Island companion without `island`** · severity `error`

> `{attribute}` only means something next to `island`, but this component has none, so it would reach the component as a stray prop. Add `island` or remove it.

`data` keys: `attribute`

Automatic fix: no

`islandMedia`, `islandRootMargin` and `islandFallback` are consumed by the compiler and never reach the component. Without `island` there is no island to consume them. `data.attribute` is the name.

**Repair:** Add `island="…"` to the component, or remove the companion.

Before:

```tsx
import { Counter } from "./Counter";

export const view = <Counter islandMedia="(max-width: 40rem)" />;
```

After:

```tsx
import { Counter } from "./Counter";

export const view = <Counter island="media" islandMedia="(max-width: 40rem)" />;
```

## MISSING_MODULE_ID

**Build target without a module id** · severity `error`

> The `{target}` target keys its output by module, but no `moduleId` was passed, so this file was not compiled. Pass the canonical module id for this file.

`data` keys: `target`

Automatic fix: no

`hydrate` and `html` outputs are keyed by module: compiler sites, hydration ranges and payloads all reference the canonical module id the build driver passes. An absent or empty `moduleId` would make those keys unstable, so compilation stops before parsing. The `client` target needs no `moduleId`. `data.target` is the requested target.

**Repair:** Pass `moduleId` alongside `target`, for example `compile(source, filename, { target: "hydrate", moduleId: "src/view.tsx" })`. The id itself is opaque to the compiler: the build driver owns its canonical form.

Example:

```tsx
export const view = <div />;
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

Reze has no virtual DOM to reconcile by `key`: `<For>` keys its rows by item identity, or by its own `keyed` function. On a native element `key` is an ordinary attribute.

**Repair:** Apply the fix to remove the attribute. To key rows, render the list with `<For each={items()} keyed={(item) => item.id}>`.

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

## PROPS_DESTRUCTURED

**Props destructured in a form that cannot be rewritten** · severity `warn`

> `{component}` destructures its props in a form the compiler cannot rewrite ({reason}), so each value is read once and never updates. Take `props` and read `props.x` where it is used.

`data` keys: `component`, `reason`

Automatic fix: no

Destructured props are rewritten into lazy reads of one props object, unless the pattern uses a computed key (`computed-key`), a default that cannot run once at the start (`default`), a default on a nested pattern (`nested-default`), a nested rest (`nested-rest`), an array pattern (`array-pattern`), a name that is assigned (`written`), `arguments` (`arguments`), a generator (`generator`), more than one parameter (`params`), or a type-position read of a non-identifier key (`type`), which no qualified type name can spell. `data.component` is the component and `data.reason` the rule.

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

export const view = <ul><For each={[1, 2, 3]}>{(n) => <li>{n}</li>}</For></ul>;
```

After:

```tsx
import { For } from "reze-js";

const numbers = [1, 2, 3];
export const view = <ul><For each={numbers}>{(n) => <li>{n}</li>}</For></ul>;
```

## ASYNC_COMPONENT_SHAPE

**Async component in a form that cannot be compiled** · severity `warn`

> `{component}` is an async component the compiler cannot rewrite ({reason}), so it stays an `async` function that returns a Promise and renders nothing. Reshape the awaits so each one is a whole statement.

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

## SIGNAL_READ_ONCE

**`signal` or `computed` copied once in a component body** · severity `warn`

> `{variable}` copies `{signal}` once, when the component runs, and never updates. Read `{signal}` where the value is used, or derive it with `computed(…)`.

`data` keys: `signal`, `variable`

Automatic fix: no

A component function runs once, so a `signal` or `computed` read directly in the initializer of one of its top-level variables is read once and the variable keeps that value. Reads inside functions, `computed` and JSX are reactive and are not reported. `data.signal` is the variable read and `data.variable` the declared variable.

**Repair:** Move the read to where the value is used, or declare `const name = computed(expression)` and read `name`. Mark an intended one-time read with `untrack(() => …)`.

Before:

```tsx
import { signal } from "reze-js";

export function Counter() {
  let count = signal(0);
  const doubled = count * 2;
  return <button onClick={() => (count += 1)}>{doubled}</button>;
}
```

After:

```tsx
import { computed, signal } from "reze-js";

export function Counter() {
  let count = signal(0);
  const doubled = computed(count * 2);
  return <button onClick={() => (count += 1)}>{doubled}</button>;
}
```

## ACTION_NESTED_WRITE

**Write in a function of `action` that runs later** · severity `warn`

> This write is in {via} inside an `action` body, which runs after the action moved on, so it is not undone when the action fails. Await the value and write it in the body.

`data` keys: `via`

Automatic fix: no

Only the `action` body is kept inside the action across its `await`s; nested functions are not rewritten. A write to a member in an async function, or in a callback of `.then`, `.catch`, `.finally`, `setTimeout`, `setInterval`, `queueMicrotask`, `requestAnimationFrame` or `requestIdleCallback`, likely runs outside the action and is a real write. The compiler cannot tell a store from a plain object, so this is a heuristic. `data.via` is the enclosing function.

**Repair:** Replace `.then((saved) => { todo.at = saved.at; })` with `const saved = await …; todo.at = saved.at;` in the body. A write outside the action on purpose can stay; move it out of the body to silence the warning.

Before:

```tsx
import { action } from "reze-js";

export const save = action(async (todo) => {
  todo.done = true;
  api.save(todo).then((saved) => {
    todo.at = saved.at;
  });
});
```

After:

```tsx
import { action } from "reze-js";

export const save = action(async (todo) => {
  todo.done = true;
  const saved = await api.save(todo);
  todo.at = saved.at;
});
```

## ISLAND_NOT_SPLIT

**Island component that stays in the main chunk** · severity `warn`

> `{component}` is an island, but it is also {reason}, so it loads from the main chunk instead of its own. Only its execution waits for the trigger.

`data` keys: `component`, `reason`

Automatic fix: no

An island splits into its own chunk when the component is imported and every use of the import is an island: the static import is then replaced by a dynamic one. Otherwise the component stays where it is and the island only defers its execution. `data.component` is the tag and `data.reason` is `used outside islands` or `exported`.

**Repair:** Move the component to its own module, import it where the island is, and use the import only as an island.

Before:

```tsx
import { Counter } from "./Counter";

export const first = <Counter island="visible" />;
export const second = <Counter step={1} />;
```

After:

```tsx
import { Counter } from "./Counter";

export const view = <Counter island="visible" />;
```

## SIGNAL_FOLDED

**Constant signal folded** · severity `info`

> `{signal}` is never written, so it compiled to a plain constant.

`data` keys: `signal`

Automatic fix: no

The signal is never written and is not exported, so it became a constant: reads cost nothing and literal values render straight into the template. `data.signal` is the variable.

**Repair:** Nothing to repair. Write the variable somewhere and the fold disappears.

Example:

```tsx
import { signal } from "reze-js";

const title = signal("Reze");
export const view = <h1>{title}</h1>;
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

> Each row compares `{signal}` with its own key, so a change would re-run every row; the comparison now reads a selector created once for this `<For>`.

`data` keys: `signal`

Automatic fix: no

A `<For>` row compares a `signal`/`computed` declared outside the row with a key built from the row's parameters. One `selector` per `<For>` replaces the comparison, so a change re-runs only the rows whose result flips. `data.signal` is the variable.

**Repair:** Nothing to repair. Compare inside a nested function, or with something that is not the row's key, and it stays a plain comparison.

Example:

```tsx
import { For, signal } from "reze-js";

let selected = signal(0);
export const view = (
  <ul>
    <For each={rows()}>
      {(row) => <li class={selected === row.id ? "on" : ""} onClick={() => (selected = row.id)} />}
    </For>
  </ul>
);
```
