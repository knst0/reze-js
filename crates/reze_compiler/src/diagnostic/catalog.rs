use super::Severity;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Code {
    ParseError,
    ClassAlias,
    ChildrenPropIgnored,
    KeyOnElement,
    DuplicateAttribute,
    UnknownAttribute,
    EventNameLowercase,
    SignalNotCalled,
    PropsDestructured,
    InlineEach,
    AsyncComponentShape,
    AsyncReturnType,
    SignalFolded,
    DeadBranchRemoved,
    ComputedInlined,
    AutoSelector,
    ShowInlined,
    PropFolded,
    PropsRewritten,
    StoreUnproxied,
    StaticComponent,
    ClientComponent,
    Island,
    LazyIsland,
    IslandDirectiveIgnored,
    FactsStale,
    ProgramOpenImport,
    FeatureFlagMismatch,
}

pub struct Entry {
    pub code: Code,
    pub name: &'static str,
    pub severity: Severity,
    pub title: &'static str,
    pub observed: &'static str,
    pub repair: &'static str,
    pub bad: &'static str,
    pub good: &'static str,
}

impl Code {
    pub fn entry(self) -> &'static Entry {
        CATALOG.iter().find(|entry| entry.code == self).expect("every code has a catalog entry")
    }

    pub fn name(self) -> &'static str {
        self.entry().name
    }

    pub fn severity(self) -> Severity {
        self.entry().severity
    }

    pub fn from_name(name: &str) -> Option<Code> {
        CATALOG.iter().find(|entry| entry.name == name).map(|entry| entry.code)
    }
}

impl serde::Serialize for Code {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> serde::Deserialize<'de> for Code {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        Code::from_name(&name)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown diagnostic code `{name}`")))
    }
}

pub const DOCS_BASE: &str = "https://github.com/knst0/reze-js/blob/main/packages/compiler/skills/compiler-diagnostics/SKILL.md";

pub const SKILL_PATH: &str = "node_modules/@rezejs/compiler/skills/compiler-diagnostics/SKILL.md";

pub const CATALOG: &[Entry] = &[
    Entry {
        code: Code::ParseError,
        name: "PARSE_ERROR",
        severity: Severity::Error,
        title: "The file does not parse",
        observed: "The parser rejected the source; nothing was compiled.",
        repair: "Fix the syntax at the reported position. The message is the parser's own. JSX-specific causes: an unclosed tag, `{` without `}`, or JSX in a `.ts` file (rename it to `.tsx`).",
        bad: "const a = <div>;",
        good: "const a = <div />;",
    },
    Entry {
        code: Code::ClassAlias,
        name: "CLASS_ALIAS",
        severity: Severity::Warn,
        title: "`className` / `classList` instead of `class`",
        observed: "A native element uses `className` or `classList`. Reze has one class attribute, `class`, which accepts a string, a toggle object, or a (nested) array of both. The compiler compiled the alias as `class` and merged it with any other class sources on the element.",
        repair: "Apply the fix: rename the attribute to `class`. When the element has several class sources, merge them into one array: `class={[\"btn\", { active: on() }]}`.",
        bad: "<button className=\"btn\" classList={{ active: on() }} />",
        good: "<button class={[\"btn\", { active: on() }]} />",
    },
    Entry {
        code: Code::ChildrenPropIgnored,
        name: "CHILDREN_PROP_IGNORED",
        severity: Severity::Warn,
        title: "`children` attribute next to nested children",
        observed: "An element has both a `children` attribute and nested JSX children. Nested children win; the attribute is never rendered.",
        repair: "Apply the fix to remove the attribute, or move its value between the tags.",
        bad: "<div children={a()}><b /></div>",
        good: "<div>{a()}<b /></div>",
    },
    Entry {
        code: Code::KeyOnElement,
        name: "KEY_ON_ELEMENT",
        severity: Severity::Warn,
        title: "`key` on a native element",
        observed: "`key` has no meaning in Reze: list rows are keyed by item identity in `<For>`. On a native element it renders as a useless `key` attribute.",
        repair: "Apply the fix to remove the attribute. To key rows, render the list with `<For each={items()}>`.",
        bad: "{items().map((item) => <li key={item.id}>{item.name}</li>)}",
        good: "<For each={items()}>{(item) => <li>{item.name}</li>}</For>",
    },
    Entry {
        code: Code::DuplicateAttribute,
        name: "DUPLICATE_ATTRIBUTE",
        severity: Severity::Warn,
        title: "The same attribute twice on one element",
        observed: "An element sets the same attribute more than once. The last one wins; the earlier ones are dead code.",
        repair: "Apply the fix to remove the earlier attribute (marked by the secondary label), or merge both values into one.",
        bad: "<a href=\"/a\" href={url()} />",
        good: "<a href={url()} />",
    },
    Entry {
        code: Code::UnknownAttribute,
        name: "UNKNOWN_ATTRIBUTE",
        severity: Severity::Warn,
        title: "Probable attribute typo",
        observed: "The attribute is not a known HTML/SVG attribute but is one or two edits away from one. It renders exactly as written, so the browser ignores it.",
        repair: "Apply the fix to rename it to the suggestion. Custom attributes should use a `data-` prefix, which is never checked.",
        bad: "<div clas=\"box\" />",
        good: "<div class=\"box\" />",
    },
    Entry {
        code: Code::EventNameLowercase,
        name: "EVENT_NAME_LOWERCASE",
        severity: Severity::Warn,
        title: "Lower-case event attribute with a function",
        observed: "`onclick={fn}` passes a function to a lower-case `on…` attribute. Only camel-case `onClick` (or `on:click`) attaches a listener; the compiler compiled it as `onClick`.",
        repair: "Apply the fix: rename the attribute to camel case (`onClick`), or use `on:click` for a non-delegated listener.",
        bad: "<button onclick={() => save()} />",
        good: "<button onClick={() => save()} />",
    },
    Entry {
        code: Code::SignalNotCalled,
        name: "SIGNAL_NOT_CALLED",
        severity: Severity::Warn,
        title: "Signal passed without calling it",
        observed: "A signal or computed getter is passed as an attribute, property or style value without being called. The DOM receives the function itself (its source text as the attribute value), not the current value, and never updates.",
        repair: "Apply the fix: call the getter, `count()`. The compiler tracks the call and updates the attribute when the signal changes.",
        bad: "<input value={count} title={label} />",
        good: "<input value={count()} title={label()} />",
    },
    Entry {
        code: Code::PropsDestructured,
        name: "PROPS_DESTRUCTURED",
        severity: Severity::Warn,
        title: "Component props destructured in the parameter list",
        observed: "A component destructures its props in the parameters. Props are getters: destructuring reads each one once, when the component runs, so the component never sees later updates.",
        repair: "Take `props` as one parameter and read `props.name` where the value is used (in JSX, a memo or an effect). Use `splitProps` to forward a subset.",
        bad: "function Greeting({ name }) {\n  return <p>Hello {name}</p>;\n}",
        good: "function Greeting(props) {\n  return <p>Hello {props.name}</p>;\n}",
    },
    Entry {
        code: Code::InlineEach,
        name: "INLINE_EACH",
        severity: Severity::Warn,
        title: "Inline array literal in `<For each>`",
        observed: "`<For each={[…]}>` creates a new array with new identity on every evaluation, so every row is rebuilt each time the parent re-renders.",
        repair: "Hoist the array to a module constant, or hold it in a signal or memo.",
        bad: "<For each={[1, 2, 3]}>{(n) => <li>{n}</li>}</For>",
        good: "const numbers = [1, 2, 3];\n<For each={numbers}>{(n) => <li>{n}</li>}</For>",
    },
    Entry {
        code: Code::AsyncComponentShape,
        name: "ASYNC_COMPONENT_SHAPE",
        severity: Severity::Warn,
        title: "Async component outside the supported shape",
        observed: "An `async` function that renders JSX was left as written: its body is not `const x = await …;` declarations followed by `return …;`. `data.reason` names the failed rule. Left as written, it returns a Promise, which renders nothing.",
        repair: "Reshape the body: top-level `const x = await fetchX(…);` declarations (one declarator each), other statements between them allowed, no `return`/`throw` before the last `await`, a final `return <…/>;`, and no local from before an `await` used after it.",
        bad: "async function User(props) {\n  const label = props.label;\n  const user = await fetchUser(props.id);\n  return <p>{label}: {user.name}</p>;\n}",
        good: "async function User(props) {\n  const user = await fetchUser(props.id);\n  return <p>{props.label}: {user.name}</p>;\n}",
    },
    Entry {
        code: Code::AsyncReturnType,
        name: "ASYNC_RETURN_TYPE",
        severity: Severity::Warn,
        title: "Async component return type cannot be unwrapped",
        observed: "The compiled component is synchronous, so its `Promise<T>` annotation is unwrapped to `T`. This annotation names `Promise` in a form the compiler cannot unwrap and was kept, so the emitted TypeScript is wrong.",
        repair: "Annotate as `Promise<JSX.Element>` or remove the return type annotation.",
        bad: "async function User(): Promise { … }",
        good: "async function User(): Promise<JSX.Element> { … }",
    },
    Entry {
        code: Code::SignalFolded,
        name: "SIGNAL_FOLDED",
        severity: Severity::Info,
        title: "Constant signal folded",
        observed: "The signal's setter is never used and its getter is only ever called, so the compiler replaced it with a plain constant (optimization O3). Reads cost nothing and literal values render straight into the template.",
        repair: "Nothing to repair. If the value is meant to change, call its setter somewhere; the fold disappears on its own.",
        bad: "",
        good: "const [title] = signal(\"Reze\");\n<h1>{title()}</h1>",
    },
    Entry {
        code: Code::DeadBranchRemoved,
        name: "DEAD_BRANCH_REMOVED",
        severity: Severity::Info,
        title: "Dead JSX branch removed",
        observed: "A child's condition is a literal, so one branch can never render. The compiler dropped it together with its templates and runtime imports (optimization O5).",
        repair: "Nothing to repair. Delete the dead branch from the source to make the intent explicit.",
        bad: "",
        good: "{DEBUG && <DebugPanel />}",
    },
    Entry {
        code: Code::ComputedInlined,
        name: "COMPUTED_INLINED",
        severity: Severity::Info,
        title: "`computed` inlined into its only read",
        observed: "A `computed` is read exactly once, as a call inside a reactive JSX expression of the same function. The compiler removed the declaration and put its expression at the read (optimization O4): the binding reads the sources directly and still compares the value before touching the DOM. `data.scope` is `module`.",
        repair: "Nothing to repair. Read the computed a second time, or outside JSX, and it stays a node of the graph.",
        bad: "",
        good: "const doubled = computed(() => count() * 2);\n<p>{doubled()}</p>",
    },
    Entry {
        code: Code::AutoSelector,
        name: "AUTO_SELECTOR",
        severity: Severity::Info,
        title: "Row comparison compiled to a selector",
        observed: "A `<For>` row compares a `signal`/`computed` declared outside the list with a key read from the row (`selected() === row().id`). Every row would subscribe to that signal and re-run on each change. The compiler creates one `selector` per `<For>` and each row reads `isSelected(key)` instead, so a change re-runs only the rows whose result flips (optimization O6). `data.source` is the getter's name.",
        repair: "Nothing to repair. Move the comparison into a function nested in the row, or compare with something other than the row's key, and it stays a plain comparison.",
        bad: "",
        good: "<For each={rows()}>{(row) => <tr class={selected() === row().id ? \"on\" : \"\"} />}</For>",
    },
    Entry {
        code: Code::ShowInlined,
        name: "SHOW_INLINED",
        severity: Severity::Info,
        title: "`<Show>` compiled to a conditional",
        observed: "A runtime `<Show>` inside a native element has a `when`, an optional `fallback`, and one child that is not a function. The compiler compiled it like `{when ? child : fallback}`: one memo of the condition's truthiness and an insert, instead of a component with its own computeds (optimization O7). The branch is still rebuilt only when the truthiness flips.",
        repair: "Nothing to repair. A function child, other attributes or several children keep the `<Show>` component.",
        bad: "",
        good: "<div><Show when={open()} fallback={<i>closed</i>}><b>open</b></Show></div>",
    },
    Entry {
        code: Code::PropFolded,
        name: "PROP_FOLDED",
        severity: Severity::Info,
        title: "Prop folded to the literal every call site passes",
        observed: "In the whole-program build every JSX use of the component passes this prop as the same string or integer literal, without spreads, and the component is not used any other way. The compiler replaced its reads (`props.k`, or a destructured `k`) with that literal, so it folds into templates like a constant (§15.16). `related` lists the call sites; `data.prop` is the key.",
        repair: "Nothing to repair. Pass a different value at one call site, or use the component other than as a JSX tag, and the prop is read at runtime again.",
        bad: "",
        good: "<Counter step={1} />\nfunction Counter(props) { return <b>+{props.step}</b>; }",
    },
    Entry {
        code: Code::PropsRewritten,
        name: "PROPS_REWRITTEN",
        severity: Severity::Info,
        title: "Destructured props rewritten to lazy reads",
        observed: "The component destructures its props in the parameter list. The compiler replaced the pattern with one `props` parameter and every destructured name with a read of `props.name` at its use, so each read stays reactive. Defaults apply when the prop is `undefined`; a rest element becomes `splitProps`.",
        repair: "Nothing to repair.",
        bad: "",
        good: "function Greeting({ name = \"you\" }) {\n  return <p>Hello {name}</p>;\n}",
    },
    Entry {
        code: Code::StoreUnproxied,
        name: "STORE_UNPROXIED",
        severity: Severity::Info,
        title: "Store replaced by one signal per field",
        observed: "Every read of the store is a path to a field and every write goes through its setter to a field, so the compiler replaced the Proxy with one signal per field: reads are signal calls and draft writes are signal writes under `untrack`. `data.scope` is `module`, or `program` when the store is exported and every importer was rewritten too; `related` lists the uses in other modules.",
        repair: "Nothing to repair. Any other use of the store (passing it whole, a computed key, a namespace access) keeps the Proxy.",
        bad: "",
        good: "const [todo, setTodo] = store({ title: \"\", done: false });\n<input checked={todo.done} onInput={() => setTodo((d) => { d.done = !d.done; })} />",
    },
    Entry {
        code: Code::StaticComponent,
        name: "STATIC_COMPONENT",
        severity: Severity::Info,
        title: "Static component",
        observed: "Program analysis proved the component renders HTML and nothing else: it reads no reactive state, attaches no behavior, and its DOM never changes. Under an islands root it is rendered on the server only and its code is never run in the browser.",
        repair: "Nothing to repair.",
        bad: "",
        good: "export function Footer() {\n  return <footer>© Reze</footer>;\n}",
    },
    Entry {
        code: Code::ClientComponent,
        name: "CLIENT_COMPONENT",
        severity: Severity::Info,
        title: "Client component",
        observed: "The component has to run in the browser. `data.reason` and the message name the first thing that makes it so (an event handler, a signal read, a component outside the program, or a client child in a position that cannot be an island); `related` continues the chain into other components and modules.",
        repair: "Nothing to repair. To make a parent static, move the interactive part into its own exported component and render it with JSON-serializable props.",
        bad: "",
        good: "export function Counter() {\n  const [n, setN] = signal(0);\n  return <button onClick={() => setN(n() + 1)}>{n()}</button>;\n}",
    },
    Entry {
        code: Code::Island,
        name: "ISLAND",
        severity: Severity::Info,
        title: "Island boundary",
        observed: "A static component renders a client component with JSON-serializable props or JSX slots. The server marks the boundary and serializes the props; the browser hydrates only the island, with the runtime features in `data.features` and the load mode in `data.mode`. `data.id` identifies the island.",
        repair: "Nothing to repair.",
        bad: "",
        good: "export function Page() {\n  return <main><h1>Docs</h1><Counter start={1} /></main>;\n}",
    },
    Entry {
        code: Code::LazyIsland,
        name: "LAZY_ISLAND",
        severity: Severity::Info,
        title: "Island loaded lazily",
        observed: "The island boundary carries `island:load` with a mode other than `eager`: its module is split into its own chunk and loaded on `idle`, when it becomes `visible`, or on the first `interaction` (`data.mode`). Events that reach the island before its code has loaded are dropped.",
        repair: "Nothing to repair. Use `island:load=\"eager\"` (or drop the attribute) for islands that must react to the very first event.",
        bad: "",
        good: "export function Page() {\n  return <main><Comments island:load=\"visible\" post={1} /></main>;\n}",
    },
    Entry {
        code: Code::IslandDirectiveIgnored,
        name: "ISLAND_DIRECTIVE_IGNORED",
        severity: Severity::Warn,
        title: "`island:*` attribute outside an island boundary",
        observed: "An `island:load` attribute sits on an element that is not an island boundary: a native element, a component that is static or runs on the client anyway, or a build without `islands`. It is compiled as a plain attribute and has no effect on loading.",
        repair: "Remove the attribute, or make the position an island boundary (a client component rendered by a static component with JSON props or JSX slots) in a build with `islands`.",
        bad: "<Counter island:load=\"idle\" start={1} />  // inside a client component",
        good: "<Counter start={1} />",
    },
    Entry {
        code: Code::FactsStale,
        name: "FACTS_STALE",
        severity: Severity::Error,
        title: "Program facts built for a different source",
        observed: "The module was compiled with program facts whose source hash does not match the source being compiled: another plugin changed the module between the program scan and `transform`, so the cross-module decisions may be wrong.",
        repair: "Order the plugin that rewrites the module after the Reze plugin, exclude the module from `program.include`, or disable `optimize`.",
        bad: "",
        good: "reze({ program: { exclude: /generated/ } })",
    },
    Entry {
        code: Code::ProgramOpenImport,
        name: "PROGRAM_OPEN_IMPORT",
        severity: Severity::Error,
        title: "A module outside the program imports a closed module",
        observed: "Program analysis rewrote the exports of a module (a folded signal or an unproxied store) assuming it knows every importer, but a module outside the program imports it and would break.",
        repair: "Add the importing module to `program.include`, or disable `optimize` for the build.",
        bad: "",
        good: "reze({ program: { include: /\\.(tsx?|jsx?|vue)$/ } })",
    },
    Entry {
        code: Code::FeatureFlagMismatch,
        name: "FEATURE_FLAG_MISMATCH",
        severity: Severity::Error,
        title: "A module outside the program uses a disabled runtime feature",
        observed: "The program does not use a runtime feature, so its define flag was turned off and the runtime dropped it, but a module outside the program uses that feature's export.",
        repair: "Add the module to `program.include`, or force the flag on with the plugin option `features: { <name>: true }`.",
        bad: "",
        good: "reze({ features: { suspense: true } })",
    },
];

pub fn render_skill() -> String {
    let mut out = String::from(
        "# Repairing Reze compiler diagnostics\n\n\
         Generated from `crates/reze_compiler/src/diagnostic/catalog.rs`; do not edit by hand\n\
         (`REZE_UPDATE_SKILL=1 cargo test -p reze-compiler` regenerates it).\n\n\
         Every compiler diagnostic has a stable code, printed in brackets at the start of its message.\n\
         This guide maps each code to its repair.\n\n\
         ## How to act on a diagnostic\n\n\
         1. If the diagnostic carries `fixes`, apply them: each fix is a list of exact source edits\n   \
         (`start`/`end` byte offsets, replacement `text`) that removes the diagnostic.\n\
         2. Otherwise follow the repair for its code below.\n\
         3. Do not suppress or work around a diagnostic you do not understand: every `error` is broken\n   \
         code and every `warn` is code that renders wrong or wastes work. `info` codes explain an\n   \
         optimization and need no action.\n\n\
         The `in` line (`path` in JSON) names the enclosing components and elements, root first:\n\
         `in <App> › ul › <For>`. Structured output for tooling: the Vite plugin option\n\
         `diagnostics: { jsonl: \"path\" }` appends one JSON object per diagnostic.\n\n",
    );
    for entry in CATALOG {
        let severity = match entry.severity {
            Severity::Error => "error",
            Severity::Warn => "warn",
            Severity::Info => "info",
        };
        out.push_str(&format!(
            "## {}\n\n**{}** · severity `{severity}`\n\n{}\n\n**Repair:** {}\n\n",
            entry.name, entry.title, entry.observed, entry.repair
        ));
        if !entry.bad.is_empty() {
            out.push_str(&format!("Before:\n\n```tsx\n{}\n```\n\n", entry.bad));
        }
        let label = if entry.bad.is_empty() { "Example" } else { "After" };
        out.push_str(&format!("{label}:\n\n```tsx\n{}\n```\n\n", entry.good));
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}
