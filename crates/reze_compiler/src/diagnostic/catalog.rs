use super::Severity;
use Example::{Pair, Shows};
use Severity::{Error, Info, Warn};

pub enum Example {
    /// Full modules: `bad` reports the code, `good` does not.
    Pair { bad: &'static str, good: &'static str },
    /// A full module that reports the code.
    Shows(&'static str),
}

pub struct Entry {
    pub code: Code,
    pub name: &'static str,
    pub severity: Severity,
    pub title: &'static str,
    /// `{key}` placeholders are filled from the report's arguments.
    pub message: &'static str,
    pub explanation: &'static str,
    pub repair: &'static str,
    /// Title template of the automatic fix, when the code has one.
    pub fix: Option<&'static str>,
    pub example: Example,
}

macro_rules! catalog {
    ($(
        $variant:ident {
            name: $name:literal,
            severity: $severity:ident,
            title: $title:literal,
            message: $message:literal,
            explanation: $explanation:literal,
            repair: $repair:literal,
            fix: $fix:expr,
            example: $example:expr,
        }
    )*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Code {
            $($variant),*
        }

        /// Every code in declaration order: errors, then warnings, then infos.
        pub static CATALOG: &[Entry] = &[$(
            Entry {
                code: Code::$variant,
                name: $name,
                severity: $severity,
                title: $title,
                message: $message,
                explanation: $explanation,
                repair: $repair,
                fix: $fix,
                example: $example,
            }
        ),*];

        impl Code {
            pub fn entry(self) -> &'static Entry {
                match self {
                    $(Code::$variant => &CATALOG[Code::$variant as usize]),*
                }
            }

            pub fn name(self) -> &'static str {
                match self {
                    $(Code::$variant => $name),*
                }
            }

            pub fn severity(self) -> Severity {
                match self {
                    $(Code::$variant => $severity),*
                }
            }
        }
    };
}

impl serde::Serialize for Code {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

catalog! {
    ParseError {
        name: "PARSE_ERROR",
        severity: Error,
        title: "The file does not parse",
        message: "The parser rejected this file ({detail}), so nothing was compiled. Fix the syntax at this position.",
        explanation: "The parser reported a syntax error. `data.detail` is the parser's own message. Nothing of the file was compiled.",
        repair: "Fix the syntax at the reported position. JSX-specific causes: an unclosed tag, `{` without `}`, or JSX in a `.ts` file (rename it to `.tsx`).",
        fix: None,
        example: Pair {
            bad: "export const view = <div>;\n",
            good: "export const view = <div />;\n",
        },
    }
    ControlFlowAsValue {
        name: "CONTROL_FLOW_AS_VALUE",
        severity: Error,
        title: "Control-flow tag used as a value",
        message: "`{name}` is compiled away and has no runtime value, so this reference would throw. Use `{name}` only as a JSX tag.",
        explanation: "`Show`, `For`, `Repeat`, `Switch`, `Match`, `Loading`, `Errored` and `Portal` are compiler intrinsics: every `<Show>` tag compiles to direct runtime calls, and the imported function only throws. Passing the import around, calling it, or re-exporting it would reach that function at runtime. `data.name` is the intrinsic.",
        repair: "Render it as a tag, `<Show when={…}>…</Show>`. To pick a component at runtime, wrap the tag in a component of your own and select that with `dynamic(() => …)`.",
        fix: None,
        example: Pair {
            bad: "import { Show } from \"reze-js\";\n\nexport const Conditional = Show;\n",
            good: "import { Show } from \"reze-js\";\n\nexport const view = <Show when={open()}><p>open</p></Show>;\n",
        },
    }
    ControlFlowAttribute {
        name: "CONTROL_FLOW_ATTRIBUTE",
        severity: Error,
        title: "Attribute a control-flow tag does not accept",
        message: "`<{tag}>` does not accept `{attribute}`, so it would be silently dropped. Remove it.",
        explanation: "A control-flow tag accepts a fixed set of attributes: `<Show when fallback>`, `<For each fallback keyed>`, `<Repeat count fallback>`, `<Switch fallback>`, `<Match when>`, `<Loading fallback>`, `<Errored fallback>`, `<Portal mount>`. Anything else, spreads included, has no meaning. `data.tag` is the tag and `data.attribute` the attribute (`{...}` for a spread).",
        repair: "Apply the fix to remove the attribute. To key `<For>` rows by a field, use `keyed={(item) => item.id}`.",
        fix: Some("remove `{attribute}`"),
        example: Pair {
            bad: "import { Show } from \"reze-js\";\n\nexport const view = <Show when={open()} keyed><p>open</p></Show>;\n",
            good: "import { Show } from \"reze-js\";\n\nexport const view = <Show when={open()}><p>open</p></Show>;\n",
        },
    }
    ControlFlowMissing {
        name: "CONTROL_FLOW_MISSING",
        severity: Error,
        title: "Control-flow tag without its required attribute",
        message: "`<{tag}>` needs `{attribute}` to decide what to render. Add `{attribute}={…}`.",
        explanation: "`<Show>` and `<Match>` render by `when`, `<For>` by `each`, `<Repeat>` by `count`. Without it there is nothing to decide on. `data.tag` is the tag and `data.attribute` the missing attribute.",
        repair: "Add the attribute with the condition (`when`), the list (`each`) or the number of rows (`count`).",
        fix: None,
        example: Pair {
            bad: "import { Show } from \"reze-js\";\n\nexport const view = <Show fallback={<p>closed</p>}><p>open</p></Show>;\n",
            good: "import { Show } from \"reze-js\";\n\nexport const view = <Show when={open()} fallback={<p>closed</p>}><p>open</p></Show>;\n",
        },
    }
    ControlFlowChildren {
        name: "CONTROL_FLOW_CHILDREN",
        severity: Error,
        title: "Control-flow tag with children it cannot render",
        message: "`<{tag}>` expects {expected} as children, so this cannot be compiled.",
        explanation: "`<Show>`, `<Match>`, `<Loading>`, `<Errored>` and `<Portal>` need at least one child. `<For>` needs exactly one row function `(item, index) => …` and `<Repeat>` one `(index) => …`, as its child or its `children` attribute. `<Switch>` only takes `<Match>` elements. `data.tag` is the tag and `data.expected` what it takes.",
        repair: "Give the tag the children it expects: wrap `<For>` rows in `{(item) => …}`, move non-`<Match>` children of `<Switch>` into a `<Match>` or its `fallback`.",
        fix: None,
        example: Pair {
            bad: "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()}><li>item</li></For></ul>;\n",
            good: "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()}>{(item) => <li>{item}</li>}</For></ul>;\n",
        },
    }
    MatchOutsideSwitch {
        name: "MATCH_OUTSIDE_SWITCH",
        severity: Error,
        title: "`<Match>` outside `<Switch>`",
        message: "`<Match>` only works as a direct child of `<Switch>`. Move it into a `<Switch>`.",
        explanation: "`<Match>` is a case of the `<Switch>` around it; on its own, or nested deeper, there is no switch to choose it.",
        repair: "Make the `<Match>` a direct child of a `<Switch>`, or use `<Show when={…}>` for a single condition.",
        fix: None,
        example: Pair {
            bad: "import { Match } from \"reze-js\";\n\nexport const view = <div><Match when={ready()}><b>ready</b></Match></div>;\n",
            good: "import { Match, Switch } from \"reze-js\";\n\nexport const view = <div><Switch><Match when={ready()}><b>ready</b></Match></Switch></div>;\n",
        },
    }
    SignalNotDeclared {
        name: "SIGNAL_NOT_DECLARED",
        severity: Error,
        title: "`$signal` or `$computed` outside a variable declaration",
        message: "`{primitive}` is only valid as the initializer of `let name = {primitive}(…)` or `const name = {primitive}(…)`. The compiler rewrites every use of that name, so it needs the declaration to see it.",
        explanation: "`$signal(…)` and `$computed(…)` are compiler syntax, not functions: the declared variable becomes a getter (and, for a written `$signal`, a setter), and each read and write of it is rewritten. A call anywhere else, a `var` declaration, the syntax passed around as a value or re-exported has no variable to rewrite. `data.primitive` is the syntax used.",
        repair: "Declare the variable with `let name = $signal(…)` or `const name = $computed(…)` and use `name` where the value is needed. Use `signal(…)` or `computed(…)` when you need the getter as a value.",
        fix: None,
        example: Pair {
            bad: "import { $signal } from \"reze-js\";\n\nexport const view = <p>{$signal(0)}</p>;\n",
            good: "import { $signal } from \"reze-js\";\n\nlet count = $signal(0);\nexport const view = <button onClick={() => (count += 1)}>{count}</button>;\n",
        },
    }
    SignalPattern {
        name: "SIGNAL_PATTERN",
        severity: Error,
        title: "`$signal` or `$computed` destructured",
        message: "`{primitive}(…)` initializes a destructuring pattern. A reactive variable is one name: declare `const name = {primitive}(…)`.",
        explanation: "The declared name is rewritten into a getter (and, for a written `$signal`, a setter), which needs exactly one identifier to rename. `data.primitive` is the syntax used.",
        repair: "Declare one name per `$signal` or `$computed`, for example `let count = $signal(0)`, and read its fields where they are used. Use `signal(…)` to get the getter and setter as a tuple.",
        fix: None,
        example: Pair {
            bad: "import { $signal } from \"reze-js\";\n\nlet [count] = $signal([0]);\nexport const view = <p>{count}</p>;\n",
            good: "import { $signal } from \"reze-js\";\n\nlet count = $signal([0]);\nexport const view = <p>{count}</p>;\n",
        },
    }
    SignalExported {
        name: "SIGNAL_EXPORTED",
        severity: Error,
        title: "`$signal` or `$computed` exported",
        message: "`{signal}` is a `{primitive}` and cannot be exported: an importing module cannot know it is reactive, so it would read a getter function or lose updates.",
        explanation: "`$signal` and `$computed` variables exist only in the file that declares them; the compiler rewrites their uses there and nowhere else. `data.signal` is the exported variable and `data.primitive` the syntax it was declared with.",
        repair: "Keep the variable private and export a function that reads it, or export the getter of a plain `signal(…)` or `computed(…)`.",
        fix: None,
        example: Pair {
            bad: "import { $signal } from \"reze-js\";\n\nexport let count = $signal(0);\n",
            good: "import { $signal } from \"reze-js\";\n\nlet count = $signal(0);\nexport const readCount = () => count;\n",
        },
    }
    SignalAssignPattern {
        name: "SIGNAL_ASSIGN_PATTERN",
        severity: Error,
        title: "`$signal` written through a pattern",
        message: "`{signal}` is written through a destructuring pattern or a `for` loop head, which the compiler cannot turn into a setter call. Assign with `{signal} = value`.",
        explanation: "A write to a `$signal` becomes a setter call. In `[a] = list`, `({ a } = obj)` and `for (a of list)` the assignment is done by the language, not by an expression the compiler can replace. `data.signal` is the written variable.",
        repair: "Read the value into a temporary and assign it with `name = value` in a statement.",
        fix: None,
        example: Pair {
            bad: "import { $signal } from \"reze-js\";\n\nlet count = $signal(0);\nexport function pick(list) {\n  [count] = list;\n}\nexport const view = <p>{count}</p>;\n",
            good: "import { $signal } from \"reze-js\";\n\nlet count = $signal(0);\nexport function pick(list) {\n  count = list[0];\n}\nexport const view = <p>{count}</p>;\n",
        },
    }
    SignalUpdateInExpression {
        name: "SIGNAL_UPDATE_IN_EXPRESSION",
        severity: Error,
        title: "`++`/`--` on a `$signal` inside an expression",
        message: "`{signal}` is incremented or decremented as part of an expression. Use it as a statement, or write `{signal} += 1`, which is an expression with the new value.",
        explanation: "`count++` as a statement becomes `setCount(count() + 1)`. Inside a larger expression, for example `() => count++` or `use(count++)`, the value of the old `++` would change meaning, so it is not accepted. `data.signal` is the updated variable.",
        repair: "Put the update in its own statement, or use `count += 1` (value: the new count) or `count -= 1`.",
        fix: None,
        example: Pair {
            bad: "import { $signal } from \"reze-js\";\n\nlet count = $signal(0);\nexport const view = <button onClick={() => count++}>{count}</button>;\n",
            good: "import { $signal } from \"reze-js\";\n\nlet count = $signal(0);\nexport const view = <button onClick={() => { count++; }}>{count}</button>;\n",
        },
    }
    ComputedWritten {
        name: "COMPUTED_WRITTEN",
        severity: Error,
        title: "`$computed` written",
        message: "`{computed}` is a `$computed` and cannot be written: its value is derived from what its expression reads. Write to those signals instead.",
        explanation: "A `$computed` compiles to a `computed` getter, which has no setter. Assignments, compound assignments, `++`/`--` and writes through a pattern or a `for` loop head are refused. `data.computed` is the written variable.",
        repair: "Write the `$signal` the expression reads, or declare the variable with `$signal` when it is meant to be set directly.",
        fix: None,
        example: Pair {
            bad: "import { $computed, $signal } from \"reze-js\";\n\nlet count = $signal(0);\nconst doubled = $computed(count * 2);\nexport const view = <button onClick={() => (doubled = 0)}>{doubled}</button>;\n",
            good: "import { $computed, $signal } from \"reze-js\";\n\nlet count = $signal(0);\nconst doubled = $computed(count * 2);\nexport const view = <button onClick={() => (count = 0)}>{doubled}</button>;\n",
        },
    }
    ComputedFunction {
        name: "COMPUTED_FUNCTION",
        severity: Error,
        title: "Function literal passed to `$computed`",
        message: "`$computed` takes the expression itself and wraps it in a function, so a function literal here would make the derived value that function. Write `$computed(expression)` without `() =>`.",
        explanation: "`const doubled = $computed(count * 2)` compiles to `computed(() => count() * 2)`. A function literal as the argument is refused rather than quietly deriving a function. A derived value that is a function comes from another expression: a call, a conditional or an identifier.",
        repair: "Apply the fix to remove `() =>`. For several statements, move them into a function and derive its call: `$computed(compute())`.",
        fix: Some("remove `() =>`"),
        example: Pair {
            bad: "import { $computed, $signal } from \"reze-js\";\n\nlet count = $signal(0);\nconst doubled = $computed(() => count * 2);\nexport const view = <button onClick={() => (count += 1)}>{doubled}</button>;\n",
            good: "import { $computed, $signal } from \"reze-js\";\n\nlet count = $signal(0);\nconst doubled = $computed(count * 2);\nexport const view = <button onClick={() => (count += 1)}>{doubled}</button>;\n",
        },
    }
    ComputedAwait {
        name: "COMPUTED_AWAIT",
        severity: Error,
        title: "`await` or `yield` in `$computed`",
        message: "The expression of `$computed` awaits or yields, but it runs inside a synchronous getter where `await` and `yield` are not valid. Derive from a value that is already loaded, or use `asyncComputed`.",
        explanation: "`$computed(expression)` compiles to `computed(() => expression)`. The getter is an ordinary arrow function, so an `await` or `yield` outside a nested function cannot move into it.",
        repair: "Load the value with `asyncComputed(() => load(…))` and derive from its `value()`, or await it in an async component before the derivation.",
        fix: None,
        example: Pair {
            bad: "import { $computed, $signal } from \"reze-js\";\n\nlet id = $signal(1);\nconst label = $computed(await describe(id));\nexport const view = <p onClick={() => (id += 1)}>{label}</p>;\n",
            good: "import { $computed, $signal, asyncComputed } from \"reze-js\";\n\nlet id = $signal(1);\nconst described = asyncComputed(() => describe(id));\nconst label = $computed(described.value() ?? \"…\");\nexport const view = <p onClick={() => (id += 1)}>{label}</p>;\n",
        },
    }
    ActionArgument {
        name: "ACTION_ARGUMENT",
        severity: Error,
        title: "`$action` without a function literal",
        message: "`$action` needs its body written at the call, as an arrow function or function expression, so the compiler can keep the action current across each `await`. Write `$action(async (…) => { … })`.",
        explanation: "`$action(fn)` compiles to `action(fn)` with `fn` rewritten: it takes the run as its first parameter, every `await` in its body resumes the run, and the body ends it. An identifier, a call or a missing argument hides the body from the compiler.",
        repair: "Write the body in the call and call the existing function from it: `$action(async (todo) => { await save(todo); })`. To thread the run by hand, use `action((run, …) => …)`.",
        fix: None,
        example: Pair {
            bad: "import { $action } from \"reze-js\";\n\nexport const save = $action(saveTodo);\n",
            good: "import { $action } from \"reze-js\";\n\nexport const save = $action(async (todo) => {\n  await saveTodo(todo);\n});\n",
        },
    }
    ActionUnsupported {
        name: "ACTION_UNSUPPORTED",
        severity: Error,
        title: "`for await`, `await using` or a generator in `$action`",
        message: "`{construct}` suspends the `$action` body where the compiler cannot resume the action, so writes after it would escape the action. Use a plain `await` instead.",
        explanation: "The compiler resumes the action after each `await` expression. `for await` and `await using` suspend without one, and a generator suspends at every `yield`. `data.construct` is what was found: `for await`, `await using` or `generator`.",
        repair: "Loop with `for (…) { const item = await next(); … }`, dispose with `try { … } finally { await resource[Symbol.asyncDispose](); }`, and pass an `async` function instead of a generator.",
        fix: None,
        example: Pair {
            bad: "import { $action } from \"reze-js\";\n\nexport const load = $action(async (list) => {\n  for await (const item of stream()) list.push(item);\n});\n",
            good: "import { $action } from \"reze-js\";\n\nexport const load = $action(async (list) => {\n  for (const item of await fetchAll()) list.push(item);\n});\n",
        },
    }
    ActionNotCalled {
        name: "ACTION_NOT_CALLED",
        severity: Error,
        title: "`$action` used as a value",
        message: "`$action` is compiler syntax and has no runtime value, so this reference would throw. Call it with the body: `$action(async (…) => { … })`.",
        explanation: "Only a call `$action(fn)` is rewritten. Passing `$action` around, storing it or re-exporting it would reach the function that only throws.",
        repair: "Call `$action` where the action is defined. To make actions from a function at runtime, use `action((run, …) => …)`.",
        fix: None,
        example: Pair {
            bad: "import { $action } from \"reze-js\";\n\nexport const make = $action;\n",
            good: "import { $action } from \"reze-js\";\n\nexport const save = $action(async (todo) => {\n  await saveTodo(todo);\n});\n",
        },
    }
    PropsAsValue {
        name: "PROPS_AS_VALUE",
        severity: Error,
        title: "`$props` used as a value",
        message: "`$props` is compiler syntax and has no runtime value, so this reference would throw. Call `$props.merge(…)`, `$props.splitByGroups(…)` or `$props.omit(…)` instead.",
        explanation: "Only the three `$props` calls are rewritten: dissolvable merges become object literals, the rest compiles to direct `mergeProps`, `splitProps` and `omitProps` calls. Passing `$props` around, storing it, re-exporting it or calling another method would reach a binding with no value.",
        repair: "Call one of the three where the props are handled. To merge an unknown shape at runtime, import `mergeProps` directly.",
        fix: None,
        example: Pair {
            bad: "import { $props } from \"reze-js\";\n\nexport const view = $props;\n",
            good: "import { $props } from \"reze-js\";\n\nexport const view = $props.omit(props, \"id\");\n",
        },
    }
    IslandTrigger {
        name: "ISLAND_TRIGGER",
        severity: Error,
        title: "`island` with an unknown trigger",
        message: "`{value}` is not an island trigger: `island` takes `eager`, `idle`, `visible`, `media` or `interaction`. Write `island` for the default, which is `eager`, or `island=\"visible\"`.",
        explanation: "`island` marks a component as an island: it renders its fallback and loads the component when the trigger fires. The trigger is read at compile time, so it must be a bare attribute or a string literal. `data.value` is what was written.",
        repair: "Write `island` or one of the five triggers as a string literal. A trigger chosen at runtime is not supported.",
        fix: None,
        example: Pair {
            bad: "import { Counter } from \"./Counter\";\n\nexport const view = <Counter island=\"sometimes\" />;\n",
            good: "import { Counter } from \"./Counter\";\n\nexport const view = <Counter island=\"visible\" />;\n",
        },
    }
    IslandMediaMissing {
        name: "ISLAND_MEDIA_MISSING",
        severity: Error,
        title: "`island=\"media\"` without `islandMedia`",
        message: "`island=\"media\"` loads when a media query matches, so it needs `islandMedia=\"(…)\"` with the query. Add it.",
        explanation: "A `media` island never observes an element: it loads when the query matches, immediately when it already does. Without the query there is nothing to match.",
        repair: "Add `islandMedia=\"(max-width: 40rem)\"` next to `island=\"media\"`.",
        fix: None,
        example: Pair {
            bad: "import { Counter } from \"./Counter\";\n\nexport const view = <Counter island=\"media\" />;\n",
            good: "import { Counter } from \"./Counter\";\n\nexport const view = <Counter island=\"media\" islandMedia=\"(max-width: 40rem)\" />;\n",
        },
    }
    IslandOnElement {
        name: "ISLAND_ON_ELEMENT",
        severity: Error,
        title: "`island` on a native element",
        message: "`island` marks a component, but this is a native element, so there is no component to defer. Move `island` to a component.",
        explanation: "Only a component tag takes `island` and its companions `islandMedia`, `islandRootMargin` and `islandFallback`. On a native element they would render as useless attributes.",
        repair: "Move the attributes to the component that should load later.",
        fix: None,
        example: Pair {
            bad: "export const view = <section island=\"visible\" />;\n",
            good: "import { Counter } from \"./Counter\";\n\nexport const view = <Counter island=\"visible\" />;\n",
        },
    }
    IslandOrphan {
        name: "ISLAND_ORPHAN",
        severity: Error,
        title: "Island companion without `island`",
        message: "`{attribute}` only means something next to `island`, but this component has none, so it would reach the component as a stray prop. Add `island` or remove it.",
        explanation: "`islandMedia`, `islandRootMargin` and `islandFallback` are consumed by the compiler and never reach the component. Without `island` there is no island to consume them. `data.attribute` is the name.",
        repair: "Add `island=\"…\"` to the component, or remove the companion.",
        fix: None,
        example: Pair {
            bad: "import { Counter } from \"./Counter\";\n\nexport const view = <Counter islandMedia=\"(max-width: 40rem)\" />;\n",
            good: "import { Counter } from \"./Counter\";\n\nexport const view = <Counter island=\"media\" islandMedia=\"(max-width: 40rem)\" />;\n",
        },
    }
    MissingModuleId {
        name: "MISSING_MODULE_ID",
        severity: Error,
        title: "Build target without a module id",
        message: "The `{target}` target keys its output by module, but no `moduleId` was passed, so this file was not compiled. Pass the canonical module id for this file.",
        explanation: "`hydrate` and `html` outputs are keyed by module: compiler sites, hydration ranges and payloads all reference the canonical module id the build driver passes. An absent or empty `moduleId` would make those keys unstable, so compilation stops before parsing. The `client` target needs no `moduleId`. `data.target` is the requested target.",
        repair: "Pass `moduleId` alongside `target`, for example `compile(source, filename, { target: \"hydrate\", moduleId: \"src/view.tsx\" })`. The id itself is opaque to the compiler: the build driver owns its canonical form.",
        fix: None,
        example: Shows("export const view = <div />;\n"),
    }
    ChildrenPropIgnored {
        name: "CHILDREN_PROP_IGNORED",
        severity: Warn,
        title: "`children` attribute next to nested children",
        message: "`children` is set both as an attribute and as nested children, and nested children win, so the attribute is never rendered. Remove the attribute.",
        explanation: "An element or component has both a `children` attribute and nested children. The nested children are rendered; the attribute is dead code.",
        repair: "Apply the fix to remove the attribute, or move its value between the tags.",
        fix: Some("remove the `children` attribute"),
        example: Pair {
            bad: "export const view = <div children={label()}><b /></div>;\n",
            good: "export const view = <div>{label()}<b /></div>;\n",
        },
    }
    KeyOnElement {
        name: "KEY_ON_ELEMENT",
        severity: Warn,
        title: "`key` on a native element",
        message: "`key` does nothing on a native element and renders as a useless attribute, since list rows are keyed by `<For>`. Remove it.",
        explanation: "Reze has no virtual DOM to reconcile by `key`: `<For>` keys its rows by item identity, or by its own `keyed` function. On a native element `key` is an ordinary attribute.",
        repair: "Apply the fix to remove the attribute. To key rows, render the list with `<For each={items()} keyed={(item) => item.id}>`.",
        fix: Some("remove `key`"),
        example: Pair {
            bad: "export const view = <li key=\"a\">a</li>;\n",
            good: "export const view = <li>a</li>;\n",
        },
    }
    DuplicateAttribute {
        name: "DUPLICATE_ATTRIBUTE",
        severity: Warn,
        title: "The same attribute twice on one element",
        message: "`{attribute}` is set twice on this element and the last one wins, so the earlier one is dead code. Remove it or merge the values.",
        explanation: "An element sets the same attribute more than once. Only the last value is used; the label marks the overridden one. `data.attribute` is the name.",
        repair: "Apply the fix to remove the earlier attribute, or merge both values into one.",
        fix: Some("remove the earlier `{attribute}`"),
        example: Pair {
            bad: "export const view = <a href=\"/a\" href={url()} />;\n",
            good: "export const view = <a href={url()} />;\n",
        },
    }
    UnknownAttribute {
        name: "UNKNOWN_ATTRIBUTE",
        severity: Warn,
        title: "Probable attribute typo",
        message: "`{attribute}` is not a known attribute and renders as written, so the browser ignores it. Did you mean `{suggestion}`?",
        explanation: "The attribute is not a known HTML or SVG attribute but is one or two edits away from one, or it is React's `className`/`classList`. It renders exactly as written. `data.attribute` is the name and `data.suggestion` the known one.",
        repair: "Apply the fix to rename it to the suggestion. Custom attributes take a `data-` prefix, which is never checked.",
        fix: Some("rename to `{suggestion}`"),
        example: Pair {
            bad: "export const view = <div clas=\"box\" />;\n",
            good: "export const view = <div class=\"box\" />;\n",
        },
    }
    EventNameLowercase {
        name: "EVENT_NAME_LOWERCASE",
        severity: Warn,
        title: "Lower-case event attribute with a function",
        message: "`{attribute}` passes a function to a lower-case attribute, which never attaches a listener. Rename it to `{suggestion}`, which is how it was compiled.",
        explanation: "Only camel-case `onClick` (or `on:click`) attaches a listener; a lower-case `onclick` is an HTML attribute that takes code as a string. The compiler compiled it as the camel-case name. `data.attribute` is the name and `data.suggestion` the camel-case one.",
        repair: "Apply the fix to rename the attribute to camel case, or use `on:click` for a listener that is not delegated.",
        fix: Some("rename to `{suggestion}`"),
        example: Pair {
            bad: "export const view = <button onclick={() => save()} />;\n",
            good: "export const view = <button onClick={() => save()} />;\n",
        },
    }
    SignalNotCalled {
        name: "SIGNAL_NOT_CALLED",
        severity: Warn,
        title: "Signal passed without calling it",
        message: "`{signal}` is a signal getter passed without calling it, so the DOM receives the function and never updates. Call it: `{signal}()`.",
        explanation: "A `signal` or `computed` getter is an attribute, property or style value without being called. The DOM receives the function itself, not the current value. Children and event handlers take functions legitimately and are not checked. `data.signal` is the getter.",
        repair: "Apply the fix to call the getter. The call is tracked, so the attribute follows the signal.",
        fix: Some("call `{signal}()`"),
        example: Pair {
            bad: "import { signal } from \"reze-js\";\n\nconst [count, setCount] = signal(0);\nexport const view = <input value={count} onInput={() => setCount(1)} />;\n",
            good: "import { signal } from \"reze-js\";\n\nconst [count, setCount] = signal(0);\nexport const view = <input value={count()} onInput={() => setCount(1)} />;\n",
        },
    }
    PropsDestructured {
        name: "PROPS_DESTRUCTURED",
        severity: Warn,
        title: "Props destructured in a form that cannot be rewritten",
        message: "`{component}` destructures its props in a form the compiler cannot rewrite ({reason}), so each value is read once and never updates. Take `props` and read `props.x` where it is used.",
        explanation: "Destructured props are rewritten into lazy reads of one props object, unless the pattern uses a computed key (`computed-key`), a default that cannot run once at the start (`default`), a default on a nested pattern (`nested-default`), a nested rest (`nested-rest`), an array pattern (`array-pattern`), a name that is assigned (`written`), `arguments` (`arguments`), a generator (`generator`), or more than one parameter (`params`). `data.component` is the component and `data.reason` the rule.",
        repair: "Take `props` as the one parameter and read `props.name` where the value is used. Use `splitProps` to forward a subset.",
        fix: None,
        example: Pair {
            bad: "export function Greeting({ name = fallback() }) {\n  return <p>Hello {name}</p>;\n}\n",
            good: "export function Greeting(props) {\n  return <p>Hello {props.name ?? fallback()}</p>;\n}\n",
        },
    }
    InlineEach {
        name: "INLINE_EACH",
        severity: Warn,
        title: "Inline array literal in `<For each>`",
        message: "`<For each={[…]}>` builds a new array on every evaluation, so every row is rebuilt each time. Hoist the array to a constant or keep it in a signal.",
        explanation: "An array literal has new identity, and new items, each time `each` is evaluated, so `<For>` cannot reuse any row.",
        repair: "Hoist the array to a module constant, or hold it in a signal or computed.",
        fix: None,
        example: Pair {
            bad: "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={[1, 2, 3]}>{(n) => <li>{n}</li>}</For></ul>;\n",
            good: "import { For } from \"reze-js\";\n\nconst numbers = [1, 2, 3];\nexport const view = <ul><For each={numbers}>{(n) => <li>{n}</li>}</For></ul>;\n",
        },
    }
    AsyncComponentShape {
        name: "ASYNC_COMPONENT_SHAPE",
        severity: Warn,
        title: "Async component in a form that cannot be compiled",
        message: "`{component}` is an async component the compiler cannot rewrite ({reason}), so it stays an `async` function that returns a Promise and renders nothing. Reshape the awaits or move the work into an `asyncComputed`.",
        explanation: "An `async` component compiles to a load step, re-run when a source it reads changes, and a body that runs once after the first load and reads each awaited value through a getter, so later loads update it in place. That needs each top-level `await` to be a whole statement, `const x = await …;` or `await …;` (`await-position`, and `nested-await` when its operand awaits again); between the first and last await only `const`/`let`/`var` declarations may appear (`statement-between-awaits`); nothing up to the last await may `return` (`return-before-await`) or contain JSX (`jsx-before-await`); no value an await produces may be assigned after the last await (`value-reassigned`). `data.component` is the component and `data.reason` the rule.",
        repair: "Give every await its own `const x = await …;` statement, keep other statements before the first await or after the last one, and start the JSX after the last await.",
        fix: None,
        example: Pair {
            bad: "export async function Card(props) {\n  const user = await fetchUser(props.id);\n  log(user);\n  const posts = await fetchPosts(user.id);\n  return <p>{posts.length}</p>;\n}\n",
            good: "export async function Card(props) {\n  const user = await fetchUser(props.id);\n  const posts = await fetchPosts(user.id);\n  log(user);\n  return <p>{posts.length}</p>;\n}\n",
        },
    }
    SignalReadOnce {
        name: "SIGNAL_READ_ONCE",
        severity: Warn,
        title: "`$signal` or `$computed` copied once in a component body",
        message: "`{variable}` copies `{signal}` once, when the component runs, and never updates. Read `{signal}` where the value is used, or derive it with `$computed(…)`.",
        explanation: "A component function runs once, so a `$signal` or `$computed` read directly in the initializer of one of its top-level variables is read once and the variable keeps that value. Reads inside functions, `$computed`, `computed` and JSX are reactive and are not reported. `data.signal` is the variable read and `data.variable` the declared variable.",
        repair: "Move the read to where the value is used, or declare `const name = $computed(expression)` and read `name`. Mark an intended one-time read with `untrack(() => …)`.",
        fix: None,
        example: Pair {
            bad: "import { $signal } from \"reze-js\";\n\nexport function Counter() {\n  let count = $signal(0);\n  const doubled = count * 2;\n  return <button onClick={() => (count += 1)}>{doubled}</button>;\n}\n",
            good: "import { $computed, $signal } from \"reze-js\";\n\nexport function Counter() {\n  let count = $signal(0);\n  const doubled = $computed(count * 2);\n  return <button onClick={() => (count += 1)}>{doubled}</button>;\n}\n",
        },
    }
    ActionNestedWrite {
        name: "ACTION_NESTED_WRITE",
        severity: Warn,
        title: "Write in a function of `$action` that runs later",
        message: "This write is in {via} inside a `$action` body, which runs after the action moved on, so it is not undone when the action fails. Await the value and write it in the body.",
        explanation: "Only the `$action` body is kept inside the action across its `await`s; nested functions are not rewritten. A write to a member in an async function, or in a callback of `.then`, `.catch`, `.finally`, `setTimeout`, `setInterval`, `queueMicrotask`, `requestAnimationFrame` or `requestIdleCallback`, likely runs outside the action and is a real write. The compiler cannot tell a store from a plain object, so this is a heuristic. `data.via` is the enclosing function.",
        repair: "Replace `.then((saved) => { todo.at = saved.at; })` with `const saved = await …; todo.at = saved.at;` in the body. A write outside the action on purpose can stay; move it out of the body to silence the warning.",
        fix: None,
        example: Pair {
            bad: "import { $action } from \"reze-js\";\n\nexport const save = $action(async (todo) => {\n  todo.done = true;\n  api.save(todo).then((saved) => {\n    todo.at = saved.at;\n  });\n});\n",
            good: "import { $action } from \"reze-js\";\n\nexport const save = $action(async (todo) => {\n  todo.done = true;\n  const saved = await api.save(todo);\n  todo.at = saved.at;\n});\n",
        },
    }
    IslandNotSplit {
        name: "ISLAND_NOT_SPLIT",
        severity: Warn,
        title: "Island component that stays in the main chunk",
        message: "`{component}` is an island, but it is also {reason}, so it loads from the main chunk instead of its own. Only its execution waits for the trigger.",
        explanation: "An island splits into its own chunk when the component is imported and every use of the import is an island: the static import is then replaced by a dynamic one. Otherwise the component stays where it is and the island only defers its execution. `data.component` is the tag and `data.reason` is `used outside islands` or `exported`.",
        repair: "Move the component to its own module, import it where the island is, and use the import only as an island.",
        fix: None,
        example: Pair {
            bad: "import { Counter } from \"./Counter\";\n\nexport const first = <Counter island=\"visible\" />;\nexport const second = <Counter step={1} />;\n",
            good: "import { Counter } from \"./Counter\";\n\nexport const view = <Counter island=\"visible\" />;\n",
        },
    }
    SignalFolded {
        name: "SIGNAL_FOLDED",
        severity: Info,
        title: "Constant signal folded",
        message: "`{signal}` is never written and every read is a call, so it compiled to a plain constant.",
        explanation: "The signal's setter is unused, its getter is only called, and neither is exported, so the signal became a constant: reads cost nothing and literal values render straight into the template. `data.signal` is the getter.",
        repair: "Nothing to repair. Call the setter somewhere and the fold disappears.",
        fix: None,
        example: Shows("import { signal } from \"reze-js\";\n\nconst [title] = signal(\"Reze\");\nexport const view = <h1>{title()}</h1>;\n"),
    }
    DeadBranchRemoved {
        name: "DEAD_BRANCH_REMOVED",
        severity: Info,
        title: "Dead JSX branch removed",
        message: "The condition is a literal, so this branch can never render; it was removed.",
        explanation: "A child's condition is a literal, so one branch never renders. It was dropped with its templates and runtime imports.",
        repair: "Nothing to repair. Delete the dead branch to make the intent explicit.",
        fix: None,
        example: Shows("export const view = <div>{false && <b>debug</b>}</div>;\n"),
    }
    PropsRewritten {
        name: "PROPS_REWRITTEN",
        severity: Info,
        title: "Destructured props rewritten to lazy reads",
        message: "`{component}` destructures its props, so the pattern became one props object and each destructured name a lazy read of it.",
        explanation: "Each destructured name reads the props object where it is used, so it stays reactive. Defaults apply when the prop is `undefined`; a rest element becomes `splitProps`. `data.component` is the component.",
        repair: "Nothing to repair.",
        fix: None,
        example: Shows("export function Greeting({ name }) {\n  return <p>Hello {name}</p>;\n}\n"),
    }
    AutoSelector {
        name: "AUTO_SELECTOR",
        severity: Info,
        title: "Row comparison compiled to a selector",
        message: "Each row compares `{signal}()` with its own key, so a change would re-run every row; the comparison now reads a selector created once for this `<For>`.",
        explanation: "A `<For>` row compares a `signal`/`computed` declared outside the row with a key built from the row's parameters. One `selector` per `<For>` replaces the comparison, so a change re-runs only the rows whose result flips. `data.signal` is the getter.",
        repair: "Nothing to repair. Compare inside a nested function, or with something that is not the row's key, and it stays a plain comparison.",
        fix: None,
        example: Shows("import { For, signal } from \"reze-js\";\n\nconst [selected, setSelected] = signal(0);\nexport const view = (\n  <ul>\n    <For each={rows()}>\n      {(row) => <li class={selected() === row.id ? \"on\" : \"\"} onClick={() => setSelected(row.id)} />}\n    </For>\n  </ul>\n);\n"),
    }
}
