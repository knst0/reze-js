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
