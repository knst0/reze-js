use super::Severity;
use Example::{Pair, Prose, Shows};
use Severity::{Error, Warn};

pub enum Example {
    /// Full modules: `bad` reports the code, `good` does not.
    Pair { bad: &'static str, good: &'static str },
    /// A full module that reports the code.
    Shows(&'static str),
    /// The code needs several files or build options; the text describes the case.
    Prose(&'static str),
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
        message: "The parser rejected this file ({detail}). Fix the syntax at this position.",
        explanation: "The parser reported a syntax error. `data.detail` is the parser's own message. Nothing of the project was compiled.",
        repair: "Fix the syntax at the reported position. JSX in a `.ts` file needs the `.tsx` extension.",
        fix: None,
        example: Pair {
            bad: "export default function Page() {\n  return <main>;\n}\n",
            good: "export default function Page() {\n  return <main />;\n}\n",
        },
    }
    RouteInvalid {
        name: "ROUTE_INVALID",
        severity: Error,
        title: "The route file does not map to a URL",
        message: "This route file cannot be served: {detail}.",
        explanation: "Route files map to URL patterns by their names: `index`, `[id]`, `[[opt]]`, `[...rest]`, and `(group)` folders. Two files that match the same URL, a catch-all that is not last, or a param name that is not an identifier make the route table ambiguous. `data.detail` names the problem.",
        repair: "Rename or move the file so every URL has exactly one route.",
        fix: None,
        example: Prose("`src/routes/blog/index.tsx` and `src/routes/blog.tsx` without children both match `/blog`."),
    }
    ImportUnresolved {
        name: "IMPORT_UNRESOLVED",
        severity: Error,
        title: "A relative import names no file",
        message: "`{specifier}` does not resolve to a file of the project.",
        explanation: "The compiler follows every relative import from the route modules to analyze the whole application. It tries the path as written, with `.tsx`, `.ts`, `.jsx`, and `.js`, and as a folder with an `index` file. `data.specifier` is the import as written.",
        repair: "Fix the path, or create the module.",
        fix: None,
        example: Shows("import { Card } from \"./missing\";\n\nexport default function Page() {\n  return <Card />;\n}\n"),
    }
    RouteDefaultExport {
        name: "ROUTE_DEFAULT_EXPORT",
        severity: Error,
        title: "A route module exports no component",
        message: "This route module has no default export component, so the route renders nothing. Export the page component as `default`.",
        explanation: "Every file under the routes directory is a route module. The server renders its default export: the page, or the layout for files with nested routes.",
        repair: "Add `export default function Page() { … }`, or move the file out of the routes directory, or prefix its name with `_`.",
        fix: None,
        example: Pair {
            bad: "export function Page() {\n  return <main />;\n}\n",
            good: "export default function Page() {\n  return <main />;\n}\n",
        },
    }
    JsxConditionalExpression {
        name: "JSX_CONDITIONAL_EXPRESSION",
        severity: Error,
        title: "A condition renders JSX",
        message: "A condition that renders JSX uses `<Show>` or `<Switch>`, not `{operator}`.",
        explanation: "Conditions that produce JSX are written with the control-flow components, so every target lowers them the same way, the callback narrows the value, and `&&` cannot render a stray `0`. `data.operator` is the operator found.",
        repair: "Apply the fix, or rewrite the expression as `<Show when={…} fallback={…}>…</Show>`.",
        fix: Some("rewrite with `<Show>`"),
        example: Pair {
            bad: "export default function Page(props: { user?: string }) {\n  return <main>{props.user ? <p>{props.user}</p> : <p>Guest</p>}</main>;\n}\n",
            good: "import { Show } from \"reze-js\";\n\nexport default function Page(props: { user?: string }) {\n  return (\n    <main>\n      <Show when={props.user} fallback={<p>Guest</p>}>\n        {(user) => <p>{user}</p>}\n      </Show>\n    </main>\n  );\n}\n",
        },
    }
    JsxListExpression {
        name: "JSX_LIST_EXPRESSION",
        severity: Error,
        title: "An array method renders JSX",
        message: "A list that renders JSX uses `<For>` or `<Repeat>`, not `.{method}`.",
        explanation: "Lists that produce JSX are written with `<For>` or `<Repeat>`, which state how rows are keyed. `data.method` is the array method found.",
        repair: "Apply the fix, or rewrite the expression as `<For each={…}>{(item) => …}</For>`.",
        fix: Some("rewrite with `<For>`"),
        example: Pair {
            bad: "export default function Page(props: { tags: string[] }) {\n  return <ul>{props.tags.map((tag) => <li>{tag}</li>)}</ul>;\n}\n",
            good: "import { For } from \"reze-js\";\n\nexport default function Page(props: { tags: string[] }) {\n  return (\n    <ul>\n      <For each={props.tags}>{(tag) => <li>{tag}</li>}</For>\n    </ul>\n  );\n}\n",
        },
    }
    JsxArrayMethod {
        name: "JSX_ARRAY_METHOD",
        severity: Error,
        title: "A template calls an array method",
        message: "Array methods are not called in templates: `.{method}` here. Compute the value in the loader, or in a `computed` for client state.",
        explanation: "The template stays a projection of data, so every target, the Rust server included, can lower it. `.length` is a property and stays allowed. `data.method` is the method called.",
        repair: "Move the computation into the loader and read the prepared field, or declare a `computed` for values derived from client state.",
        fix: None,
        example: Pair {
            bad: "export default function Page(props: { tags: string[] }) {\n  return <p>{props.tags.join(\", \")}</p>;\n}\n",
            good: "export default function Page(props: { tagList: string }) {\n  return <p>{props.tagList}</p>;\n}\n",
        },
    }
    JsxOutsideTemplate {
        name: "JSX_OUTSIDE_TEMPLATE",
        severity: Error,
        title: "JSX outside a component's returned template",
        message: "JSX is written only in the template a component returns, in props of components, and in control-flow callbacks.",
        explanation: "A component runs on the server only, and its template is compiled into server render code and client bindings. JSX stored in a variable or returned from a helper function has no fixed place in that template.",
        repair: "Move the markup into the returned template, or extract it into a component.",
        fix: None,
        example: Pair {
            bad: "export default function Page() {\n  const title = <h1>Hi</h1>;\n  return <main>{title}</main>;\n}\n",
            good: "function Title() {\n  return <h1>Hi</h1>;\n}\n\nexport default function Page() {\n  return (\n    <main>\n      <Title />\n    </main>\n  );\n}\n",
        },
    }
    ComponentReturn {
        name: "COMPONENT_RETURN",
        severity: Error,
        title: "A component returns more than one template",
        message: "A component returns one template, at the end of its body. Write conditions with `<Show>` or `<Switch>` inside it.",
        explanation: "The compiler gives every component one fixed template, so client bindings can locate their nodes by path. An early `return` makes the structure depend on runtime values.",
        repair: "Remove the early return and wrap the alternatives in `<Show>` or `<Switch>`.",
        fix: None,
        example: Pair {
            bad: "export default function Page(props: { ok: boolean }) {\n  if (!props.ok) return <p>No</p>;\n  return <p>Yes</p>;\n}\n",
            good: "import { Show } from \"reze-js\";\n\nexport default function Page(props: { ok: boolean }) {\n  return (\n    <Show when={props.ok} fallback={<p>No</p>}>\n      <p>Yes</p>\n    </Show>\n  );\n}\n",
        },
    }
    ControlFlowAttribute {
        name: "CONTROL_FLOW_ATTRIBUTE",
        severity: Error,
        title: "Attribute a control-flow component does not accept",
        message: "`<{tag}>` does not accept `{attribute}`. Remove it.",
        explanation: "Control-flow components accept a fixed set of attributes: `<Show when fallback>`, `<Switch fallback>`, `<Match when>`, `<For each key fallback>`, `<Repeat count fallback>`, `<Loading fallback>`, `<Errored fallback>`, `<Portal mount>`. `data.tag` is the component and `data.attribute` the attribute.",
        repair: "Remove the attribute.",
        fix: None,
        example: Pair {
            bad: "import { Show } from \"reze-js\";\n\nexport default function Page(props: { ok: boolean }) {\n  return <Show when={props.ok} each={1}><p>ok</p></Show>;\n}\n",
            good: "import { Show } from \"reze-js\";\n\nexport default function Page(props: { ok: boolean }) {\n  return <Show when={props.ok}><p>ok</p></Show>;\n}\n",
        },
    }
    ControlFlowMissing {
        name: "CONTROL_FLOW_MISSING",
        severity: Error,
        title: "A control-flow component lacks a required attribute",
        message: "`<{tag}>` needs `{attribute}`.",
        explanation: "`<Show>` and `<Match>` need `when`, `<For>` needs `each`, `<Repeat>` needs `count`. `data.tag` is the component and `data.attribute` the missing attribute.",
        repair: "Add the attribute.",
        fix: None,
        example: Pair {
            bad: "import { Show } from \"reze-js\";\n\nexport default function Page() {\n  return <Show><p>ok</p></Show>;\n}\n",
            good: "import { Show } from \"reze-js\";\n\nexport default function Page(props: { ok: boolean }) {\n  return <Show when={props.ok}><p>ok</p></Show>;\n}\n",
        },
    }
    ControlFlowChildren {
        name: "CONTROL_FLOW_CHILDREN",
        severity: Error,
        title: "Children a control-flow component cannot render",
        message: "`<{tag}>` takes {expected} as children.",
        explanation: "`<For>` and `<Repeat>` take one callback that renders a row. `<Show>` takes markup or one callback that receives the narrowed value. `<Switch>` takes only `<Match>` elements. `data.tag` is the component and `data.expected` what it accepts.",
        repair: "Write the children in the expected form.",
        fix: None,
        example: Pair {
            bad: "import { For } from \"reze-js\";\n\nexport default function Page(props: { tags: string[] }) {\n  return <ul><For each={props.tags}><li /></For></ul>;\n}\n",
            good: "import { For } from \"reze-js\";\n\nexport default function Page(props: { tags: string[] }) {\n  return <ul><For each={props.tags}>{(tag) => <li>{tag}</li>}</For></ul>;\n}\n",
        },
    }
    ControlFlowAsValue {
        name: "CONTROL_FLOW_AS_VALUE",
        severity: Error,
        title: "A compiler component used as a value",
        message: "`{name}` exists only as a JSX tag. Use it as `<{name}>`.",
        explanation: "`Show`, `Switch`, `Match`, `For`, `Repeat`, `Loading`, `Errored`, and `Portal` are compiled away and have no runtime value. `data.name` is the component.",
        repair: "Render it as a tag.",
        fix: None,
        example: Pair {
            bad: "import { Show } from \"reze-js\";\n\nexport const Conditional = Show;\n",
            good: "import { Show } from \"reze-js\";\n\nexport default function Page(props: { ok: boolean }) {\n  return <Show when={props.ok}><p>ok</p></Show>;\n}\n",
        },
    }
    UnknownComponent {
        name: "UNKNOWN_COMPONENT",
        severity: Error,
        title: "A component the compiler cannot find",
        message: "`<{name}>` does not resolve to a component declared in the project.",
        explanation: "The compiler analyzes every component it renders: their templates become server render code and client bindings. A tag must name a component declared in the project or imported from a module with source. `data.name` is the tag.",
        repair: "Import the component from a project module, or declare it.",
        fix: None,
        example: Pair {
            bad: "export default function Page() {\n  return <Missing />;\n}\n",
            good: "function Present() {\n  return <p />;\n}\n\nexport default function Page() {\n  return <Present />;\n}\n",
        },
    }
    PackageSourceMissing {
        name: "PACKAGE_SOURCE_MISSING",
        severity: Error,
        title: "A component from a package without source",
        message: "`{package}` exposes no `reze` source, so `{name}` cannot be compiled.",
        explanation: "The compiler analyzes a library component like project code, so a package publishes its source through the `reze` export condition. `data.package` is the package and `data.name` the component.",
        repair: "Use a package with a `reze` export, or write the component in the project.",
        fix: None,
        example: Pair {
            bad: "import { Dialog } from \"some-ui\";\n\nexport default function Page() {\n  return <Dialog />;\n}\n",
            good: "function Dialog() {\n  return <dialog />;\n}\n\nexport default function Page() {\n  return <Dialog />;\n}\n",
        },
    }
    SpreadChild {
        name: "SPREAD_CHILD",
        severity: Error,
        title: "A spread child in JSX",
        message: "`{...}` as a JSX child has no fixed place in the template. Render lists with `<For>`.",
        explanation: "A spread child inserts an unknown number of nodes, which no target can locate.",
        repair: "Use `<For each={…}>`.",
        fix: None,
        example: Pair {
            bad: "export default function Page(props: { items: string[] }) {\n  return <p>{...props.items}</p>;\n}\n",
            good: "import { For } from \"reze-js\";\n\nexport default function Page(props: { items: string[] }) {\n  return <p><For each={props.items}>{(item) => item}</For></p>;\n}\n",
        },
    }
    ServerHoleUnsupported {
        name: "SERVER_HOLE_UNSUPPORTED",
        severity: Error,
        title: "A template hole the server cannot lower",
        message: "{reason}. Compute the value in the loader, or make the element interactive on the client.",
        explanation: "A hole rendered on the server is a projection of params, data, context, and loop variables: field access, literals, template literals, logical and comparison operators, arithmetic, and `.length`. Calls to unanalyzed functions have no Rust lowering, and the JS server and the Rust server must render the same bytes. `data.reason` names the expression.",
        repair: "Compute the value in the loader and read the prepared field.",
        fix: None,
        example: Pair {
            bad: "export default function Page(props: { at: Date }) {\n  return <time>{props.at.toISOString()}</time>;\n}\n",
            good: "export default function Page(props: { at: string }) {\n  return <time>{props.at}</time>;\n}\n",
        },
    }
    RustRouteInvalid {
        name: "RUST_ROUTE_INVALID",
        severity: Error,
        title: "The Rust file of a route does not fit the contract",
        message: "{reason}.",
        explanation: "A `.rs` file beside a route module holds its server code: `pub async fn load(params: Params, cx: &Context) -> Result<T>`, `pub async fn guard(params: Params, request: &Parts, cx: &Context) -> Result<()>`, and mutations `pub async fn name(input: I, request: &Parts, cx: &Context) -> Result<T>`. Data types derive `reze::Data`, are `pub`, have names unique in the crate, and live in the route file or under `src/server.rs`. The file holds items only, since reze includes it into the generated crate. `data.reason` names the problem.",
        repair: "Match the signature of the contract, or move the code into the module tree of `src/server.rs`.",
        fix: None,
        example: Prose("`pub async fn load(id: u32) -> Post` in `src/routes/posts/[id].rs`, which takes no `Params` and returns no `Result`."),
    }
    ServerJsInRust {
        name: "SERVER_JS_IN_RUST",
        severity: Error,
        title: "JS server code in an application whose server is Rust",
        message: "The Rust server cannot run the JS `{name}`: write it as `pub async fn {function}` in {file}.",
        explanation: "A project with a `Cargo.toml` at the Vite root builds one server, in Rust, and that server runs no JS. Every loader, guard, and mutation is a `pub async fn` of the `.rs` file beside its route module, and the route module imports its mutations from there. `data.name` is the JS export, `data.function` the Rust function, and `data.file` the file it belongs in.",
        repair: "Move the function into the `.rs` file beside the route module, or remove `Cargo.toml` to build the JS server.",
        fix: None,
        example: Prose("`export const load = loader(…)` in `src/routes/index.tsx` of a project with a `Cargo.toml`, instead of `pub async fn load` in `src/routes/index.rs`."),
    }
    HandlerCaptureUnserializable {
        name: "HANDLER_CAPTURE_UNSERIALIZABLE",
        severity: Error,
        title: "A handler captures a value that cannot reach the browser",
        message: "`{name}` is {what}, which cannot cross from server to browser.",
        explanation: "A handler runs in the browser without its component, so every value it captures reaches the browser in the client state: signals and stores as references, data as positional JSON, project functions as code. Anything else, such as class instances or values from unanalyzed calls, cannot be encoded. `data.name` is the captured variable and `data.what` what it holds.",
        repair: "Create the value inside the handler or inside an `effect`.",
        fix: None,
        example: Pair {
            bad: "export default function Page() {\n  const socket = new WebSocket(\"/live\");\n  return <button onClick={() => socket.send(\"ping\")}>Ping</button>;\n}\n",
            good: "export default function Page() {\n  return <button onClick={() => new WebSocket(\"/live\").send(\"ping\")}>Ping</button>;\n}\n",
        },
    }
    ModuleStateServerWrite {
        name: "MODULE_STATE_SERVER_WRITE",
        severity: Error,
        title: "The server render writes module state",
        message: "`{name}` is module state, and the server render writes it here. Write it from a handler or an `effect`.",
        explanation: "A server process serves many users, so a write during the server render would leak between requests. Module state is read-only on the server, and its initializer is a constant. `data.name` is the state.",
        repair: "Move the write into a handler or an `effect`, or keep the value in loader data.",
        fix: None,
        example: Pair {
            bad: "import { signal } from \"reze-js\";\n\nlet count = signal(0);\n\nexport default function Page() {\n  count = 1;\n  return <p>{count}</p>;\n}\n",
            good: "import { signal } from \"reze-js\";\n\nlet count = signal(0);\n\nexport default function Page() {\n  return <button onClick={() => count++}>{count}</button>;\n}\n",
        },
    }
    ModuleStateInitializer {
        name: "MODULE_STATE_INITIALIZER",
        severity: Error,
        title: "Module state starts from a non-constant value",
        message: "The initializer of module state `{name}` must be a constant, because the server renders it and the state block carries nothing for it.",
        explanation: "Module state has one instance in the browser and is read-only on the server. Its initial value must be the same in every request and every build, so the compiler requires a literal. `data.name` is the state.",
        repair: "Initialize it with a literal, and write the real value from a handler, or use loader data.",
        fix: None,
        example: Pair {
            bad: "import { signal } from \"reze-js\";\n\nexport let now = signal(Date.now());\n",
            good: "import { signal } from \"reze-js\";\n\nexport let now = signal(0);\n",
        },
    }
    EventDecisionDeferred {
        name: "EVENT_DECISION_DEFERRED",
        severity: Error,
        title: "An event decision that cannot wait for the chunk",
        message: "`{method}` depends on {condition}, which the dispatcher cannot evaluate before the chunk loads.",
        explanation: "The handler's chunk loads when the user starts the interaction, so the dispatcher decides `preventDefault`, `stopPropagation`, and `stopImmediatePropagation` from markup. Unconditional calls and conditions on `key`, `code`, `button`, and the modifier flags are extracted into the markup; any other condition is not. `data.method` is the call; `data.condition` is the test it depends on, or `control flow`.",
        repair: "Move the check to a native mechanism, such as `required` or a `beforeunload` guard, or set `eager` on the element so its chunk loads before the first interaction.",
        fix: None,
        example: Pair {
            bad: "import { signal } from \"reze-js\";\n\nexport default function Page() {\n  let dirty = signal(false);\n  return <a href=\"/next\" onClick={(event) => { if (dirty) event.preventDefault(); }}>Next</a>;\n}\n",
            good: "export default function Page() {\n  return <a href=\"/next\" onClick={(event) => event.preventDefault()}>Next</a>;\n}\n",
        },
    }
    DynamicComponentUnbounded {
        name: "DYNAMIC_COMPONENT_UNBOUNDED",
        severity: Error,
        title: "The candidates of `dynamic` are not known",
        message: "The candidates of `dynamic` are not known at build time. Choose from an object literal of components.",
        explanation: "Every target renders each candidate of a dynamic component and switches between them, so the set must be finite and visible to the compiler: an object literal whose values are project components.",
        repair: "Choose from an object, `dynamic(icons[props.kind])` with `const icons = { star: Star }`.",
        fix: None,
        example: Pair {
            bad: "import { dynamic } from \"reze-js\";\n\nexport default function Page(props: { c: any }) {\n  const Widget = dynamic(props.c);\n  return <Widget />;\n}\n",
            good: "import { dynamic } from \"reze-js\";\n\nfunction A() {\n  return <p>a</p>;\n}\n\nconst parts = { a: A };\n\nexport default function Page(props: { c: \"a\" }) {\n  const Widget = dynamic(parts[props.c]);\n  return <Widget />;\n}\n",
        },
    }
    MutationInputUntyped {
        name: "MUTATION_INPUT_UNTYPED",
        severity: Error,
        title: "A mutation without an input contract",
        message: "A mutation is a public endpoint and needs a contract for its input. Annotate the input, or pass a schema.",
        explanation: "Every `mutation` is a public POST endpoint, so its input is validated at runtime. The compiler generates the validator from the input annotation; a Standard Schema passed as the first argument replaces it. `unknown`, `any`, and a missing annotation describe nothing.",
        repair: "Annotate the first parameter with the input type, or pass a schema.",
        fix: None,
        example: Pair {
            bad: "import { mutation } from \"reze-js/server\";\n\nexport const save = mutation(async (input) => input);\n",
            good: "import { mutation } from \"reze-js/server\";\n\nexport const save = mutation(async (input: { title: string }) => input.title);\n",
        },
    }
    MutationInputUnresolved {
        name: "MUTATION_INPUT_UNRESOLVED",
        severity: Error,
        title: "A data type the compiler does not lower",
        message: "{reason}. Write the type out, or pass a schema.",
        explanation: "The compiler lowers annotations syntactically: primitives, literals and their unions, object types, interfaces and aliases from project files, arrays, tuples, `Record`, `Map`, `Set`, `Date`, `Uint8Array`, `File`, discriminated unions, and the integer aliases of `reze-js/types`. `data.reason` names what it could not lower.",
        repair: "Write the object type out, use a supported type, or pass a schema.",
        fix: None,
        example: Pair {
            bad: "import { mutation } from \"reze-js/server\";\n\ninterface Todo { title: string; done: boolean }\n\nexport const save = mutation(async (input: Pick<Todo, \"title\">) => input.title);\n",
            good: "import { mutation } from \"reze-js/server\";\n\nexport const save = mutation(async (input: { title: string }) => input.title);\n",
        },
    }
    MutationIdCollision {
        name: "MUTATION_ID_COLLISION",
        severity: Error,
        title: "Two mutations share an endpoint id",
        message: "`{name}` and `{other}` hash to the same mutation id `{id}`. Rename one of them.",
        explanation: "A mutation id is base-36 of a hash of its module path and export name, so it stays the same across builds. Two mutations with the same id would share an endpoint. `data.name` and `data.other` are the mutations and `data.id` the id.",
        repair: "Rename the export or the file of one mutation.",
        fix: None,
        example: Prose("Two mutations in different files whose path and export name hash to the same id."),
    }
    MutationFormField {
        name: "MUTATION_FORM_FIELD",
        severity: Error,
        title: "A form field the mutation input does not declare",
        message: "The form posts `{field}`, which the input type of `{mutation}` does not declare.",
        explanation: "A `<form action={mutation}>` posts its named fields as the mutation input. A field the input type does not declare would be rejected or ignored. `data.field` is the field and `data.mutation` the mutation.",
        repair: "Add the field to the input type, or rename the field.",
        fix: None,
        example: Pair {
            bad: "import { mutation } from \"reze-js/server\";\n\nexport const save = mutation(async (input: { title: string }) => input.title);\n\nexport default function Page() {\n  return <form action={save}><input name=\"name\" /></form>;\n}\n",
            good: "import { mutation } from \"reze-js/server\";\n\nexport const save = mutation(async (input: { title: string }) => input.title);\n\nexport default function Page() {\n  return <form action={save}><input name=\"title\" /></form>;\n}\n",
        },
    }
    LoaderResultUntyped {
        name: "LOADER_RESULT_UNTYPED",
        severity: Error,
        title: "Client code reads loader data without a result type",
        message: "The client reads `{read}`, but the loader of `{route}` has no result type.",
        explanation: "Client state crosses the wire as positional JSON, which needs a type the compiler can lower. The compiler never asks the TypeScript checker for an inferred type. `data.read` is the read and `data.route` the route pattern.",
        repair: "Annotate the result, `loader<Post>(…)` or `async (): Promise<Post> => …`.",
        fix: None,
        example: Pair {
            bad: "import { signal } from \"reze-js\";\nimport { loader } from \"reze-js/server\";\n\nexport const load = loader(async () => ({ likes: 1 }));\n\nexport default function Page(props: { data: { likes: number } }) {\n  let likes = signal(props.data.likes);\n  return <button onClick={() => likes++}>{likes}</button>;\n}\n",
            good: "import { signal } from \"reze-js\";\nimport { loader } from \"reze-js/server\";\n\ninterface Post { likes: number }\n\nexport const load = loader<Post>(async () => ({ likes: 1 }));\n\nexport default function Page(props: { data: Post }) {\n  let likes = signal(props.data.likes);\n  return <button onClick={() => likes++}>{likes}</button>;\n}\n",
        },
    }
    LoaderParamsType {
        name: "LOADER_PARAMS_TYPE",
        severity: Error,
        title: "A params annotation names a key the pattern lacks",
        message: "The params annotation declares `{name}`, which the pattern `{route}` does not have.",
        explanation: "Params come from the URL pattern of the route. An annotation can change their type but not add keys. `data.name` is the key and `data.route` the pattern.",
        repair: "Remove the key, or rename the route file to add the param.",
        fix: None,
        example: Prose("`loader(async ({ params }: { params: { slug: string } }) => …)` in `src/routes/blog/[id].tsx`."),
    }
    DataTypeUnsupported {
        name: "DATA_TYPE_UNSUPPORTED",
        severity: Error,
        title: "Data with no wire encoding",
        message: "Client code reads `{read}`, whose type has no wire encoding: {reason}.",
        explanation: "Values that reach the browser are encoded as positional JSON, which carries primitives, literals, objects, arrays, maps, sets, dates, and bytes. `data.read` is the read, and `data.reason` names the type.",
        repair: "Convert the value in the loader to a supported type.",
        fix: None,
        example: Prose("A loader annotated `loader<{ at: Temporal.Instant }>` whose field a handler reads."),
    }
    ServerInSpa {
        name: "SERVER_IN_SPA",
        severity: Error,
        title: "Server code in a client-only application",
        message: "`target: \"spa\"` has no server to run `{name}`.",
        explanation: "A client-only application is a static `index.html` and one chunk per route. Loaders and mutations need a server. `data.name` is the export.",
        repair: "Build with a server target, or fetch the data in an `effect`.",
        fix: None,
        example: Prose("`export const load = loader(…)` in an application built with `reze({ target: \"spa\" })`."),
    }
    ClientRangeUnsupported {
        name: "CLIENT_RANGE_UNSUPPORTED",
        severity: Error,
        title: "Content client code cannot create",
        message: "Client code cannot create {what} inside a range it updates.",
        explanation: "A `<Show>`, `<Switch>`, `<For>`, or `<Repeat>` whose condition or source reads client state is rendered again in the browser. Client code creates its content from the template: elements, text, bindings, handlers, and nested ranges. Components, boundaries, `<head>`, `<Portal>`, `children`, spread attributes, computed form actions, and callback bodies with statements are rendered only on the server. `data.what` names the construct.",
        repair: "Move the construct outside the range, or inline the component's markup into the range.",
        fix: None,
        example: Pair {
            bad: "import { signal, For } from \"reze-js\";\nimport { Item } from \"./item\";\n\nexport default function Page() {\n  let items = signal([\"a\"]);\n  return (\n    <ul onClick={() => (items = [...items, \"b\"])}>\n      <For each={items}>{(item) => <Item name={item} />}</For>\n    </ul>\n  );\n}\n",
            good: "import { signal, For } from \"reze-js\";\n\nexport default function Page() {\n  let items = signal([\"a\"]);\n  return (\n    <ul onClick={() => (items = [...items, \"b\"])}>\n      <For each={items}>{(item) => <li>{item}</li>}</For>\n    </ul>\n  );\n}\n",
        },
    }
    UntrackRedundant {
        name: "UNTRACK_REDUNDANT",
        severity: Warn,
        title: "An `untrack` the compiler does not need",
        message: "This read is already untracked here; remove `untrack`.",
        explanation: "The compiler decides which reads subscribe: reads inside handlers and reads that only initialize state never do. `untrack` is an escape hatch for the reads a rule does not cover.",
        repair: "Replace `untrack(() => x)` with `x`.",
        fix: Some("remove `untrack`"),
        example: Pair {
            bad: "import { signal, untrack } from \"reze-js\";\n\nexport default function Page() {\n  let count = signal(0);\n  return <button onClick={() => console.log(untrack(() => count))}>Log</button>;\n}\n",
            good: "import { signal } from \"reze-js\";\n\nexport default function Page() {\n  let count = signal(0);\n  return <button onClick={() => console.log(count)}>Log</button>;\n}\n",
        },
    }
}
