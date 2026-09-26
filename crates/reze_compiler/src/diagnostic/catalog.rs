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
        explanation: "`Show`, `For`, `Switch` and `Match` are compiler intrinsics: every `<Show>` tag compiles to direct runtime calls, and the imported function only throws. Passing the import around, calling it, or re-exporting it would reach that function at runtime. `data.name` is the intrinsic.",
        repair: "Render it as a tag, `<Show when={…}>…</Show>`. To pick a component at runtime, wrap the tag in a component of your own and pass that.",
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
        explanation: "A control-flow tag accepts a fixed set of attributes: `<Show when fallback>`, `<For each fallback key>`, `<Switch fallback>`, `<Match when>`. Anything else, spreads included, has no meaning. `data.tag` is the tag and `data.attribute` the attribute (`{...}` for a spread).",
        repair: "Apply the fix to remove the attribute. To key `<For>` rows by a field, use `key={(item) => item.id}`.",
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
        explanation: "`<Show>` and `<Match>` render by `when`, `<For>` by `each`. Without it there is nothing to decide on. `data.tag` is the tag and `data.attribute` the missing attribute.",
        repair: "Add the attribute with the condition (`when`) or the list (`each`).",
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
        explanation: "`<Show>` and `<Match>` need at least one child. `<For>` needs exactly one row function `(item, index) => …`, as its child or its `children` attribute. `<Switch>` only takes `<Match>` elements. `data.tag` is the tag and `data.expected` what it takes.",
        repair: "Give the tag the children it expects: wrap `<For>` rows in `{(item) => …}`, move non-`<Match>` children of `<Switch>` into a `<Match>` or its `fallback`.",
        fix: None,
        example: Pair {
            bad: "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()}><li>item</li></For></ul>;\n",
            good: "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()}>{(item) => <li>{item()}</li>}</For></ul>;\n",
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
        explanation: "Reze has no virtual DOM to reconcile by `key`: `<For>` keys its rows by item identity, or by its own `key` function. On a native element `key` is an ordinary attribute.",
        repair: "Apply the fix to remove the attribute. To key rows, render the list with `<For each={items()} key={(item) => item.id}>`.",
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
            bad: "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={[1, 2, 3]}>{(n) => <li>{n()}</li>}</For></ul>;\n",
            good: "import { For } from \"reze-js\";\n\nconst numbers = [1, 2, 3];\nexport const view = <ul><For each={numbers}>{(n) => <li>{n()}</li>}</For></ul>;\n",
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
        example: Shows("import { For, signal } from \"reze-js\";\n\nconst [selected, setSelected] = signal(0);\nexport const view = (\n  <ul>\n    <For each={rows()}>\n      {(row) => <li class={selected() === row().id ? \"on\" : \"\"} onClick={() => setSelected(row().id)} />}\n    </For>\n  </ul>\n);\n"),
    }
}
