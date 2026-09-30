use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use reze_compiler::{Code, Diagnostic, Options, Severity, compile};

fn options() -> Options {
    Options { source_map: false, ..Options::default() }
}

fn output(source: &str, options: &Options) -> (String, Vec<Diagnostic>) {
    let (code, diagnostics) = match compile(source, "test.tsx", options).expect("compiles") {
        Some(out) => (out.code, out.diagnostics),
        None => (source.to_string(), Vec::new()),
    };
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &code, SourceType::tsx()).parse();
    assert!(parsed.diagnostics.is_empty(), "{:?}\n{code}", parsed.diagnostics);
    let semantic = SemanticBuilder::new().with_check_syntax_error(true).build(&parsed.program);
    assert!(semantic.diagnostics.is_empty(), "{:?}\n{code}", semantic.diagnostics);
    assert!(!code.contains("$action"), "{code}");
    (code, diagnostics)
}

fn compiled(source: &str, options: &Options) -> String {
    output(source, options).0
}

fn only(source: &str, code: Code) -> Diagnostic {
    let found: Vec<_> = match compile(source, "test.tsx", &options()) {
        Ok(out) => out.map(|out| out.diagnostics).unwrap_or_default(),
        Err(errors) => errors,
    }
    .into_iter()
    .filter(|d| d.code == code)
    .collect();
    assert_eq!(found.len(), 1, "{source}\n{found:?}");
    found.into_iter().next().unwrap()
}

/// Each pair is a `$action` module and what a person writes with `action`; both must compile to
/// the same text.
const INVARIANT: &[(&str, &str, &str)] = &[
    (
        "awaits_between_writes",
        "import { $action } from \"reze-js\";\nexport const toggle = $action(async (todo) => {\n  todo.done = !todo.done;\n  const saved = await api.toggle(todo.id);\n  todo.updatedAt = saved.updatedAt;\n  await api.log(saved);\n});",
        "import { action } from \"reze-js\";\nexport const toggle = action(async (_a$, todo) => { try {\n  todo.done = !todo.done;\n  const saved = _a$.resume(await _a$.suspend(api.toggle(todo.id)));\n  todo.updatedAt = saved.updatedAt;\n  _a$.resume(await _a$.suspend(api.log(saved)));\n} finally { _a$.end(); } });",
    ),
    (
        "nested_awaits_and_return_await",
        "import { $action } from \"reze-js\";\nexport const load = $action(async () => {\n  return await f(await g(), await await h);\n});",
        "import { action } from \"reze-js\";\nexport const load = action(async (_a$) => { try {\n  return _a$.resume(await _a$.suspend(f(_a$.resume(await _a$.suspend(g())), _a$.resume(await _a$.suspend(_a$.resume(await _a$.suspend(h)))))));\n} finally { _a$.end(); } });",
    ),
    (
        "expression_bodies_and_a_bare_parameter",
        "import { $action } from \"reze-js\";\nexport const save = $action(async x => await put(x));\nexport const pick = $action((list) => ({ first: list[0] }));",
        "import { action } from \"reze-js\";\nexport const save = action(async (_a$, x) => { try { return _a$.resume(await _a$.suspend(put(x))); } finally { _a$.end(); } });\nexport const pick = action((_a$, list) => { try { return ({ first: list[0] }); } finally { _a$.end(); } });",
    ),
    (
        "try_catch_finally",
        "import { $action } from \"reze-js\";\nexport const save = $action(async (t) => {\n  try {\n    await put(t);\n  } catch (e) {\n    t.error = e;\n  } finally {\n    t.busy = false;\n  }\n  try {\n    t.a = 1;\n  } catch {\n    await report();\n  } finally {\n    t.b = 1;\n  }\n  try {\n    t.c = 1;\n  } finally {\n    t.d = 1;\n  }\n});",
        "import { action } from \"reze-js\";\nexport const save = action(async (_a$, t) => { try {\n  try {\n    _a$.resume(await _a$.suspend(put(t)));\n  } catch (e) { _a$.resume();\n    t.error = e;\n  } finally { _a$.resume();\n    t.busy = false;\n  }\n  try {\n    t.a = 1;\n  } catch {\n    _a$.resume(await _a$.suspend(report()));\n  } finally { _a$.resume();\n    t.b = 1;\n  }\n  try {\n    t.c = 1;\n  } finally {\n    t.d = 1;\n  }\n} finally { _a$.end(); } });",
    ),
    (
        "parameter_forms",
        "import { $action } from \"reze-js\";\nexport const a = $action(async ({ id }, [first] = [], ...rest) => { await put(id, first, rest); });\nexport const b = $action(async function (this: Window, n: number = 1) { await wait(n); });\nexport const c = $action(function named() { done(); });\nexport const d = $action(async <T,>(value: T): Promise<void> => { await put(value); });",
        "import { action } from \"reze-js\";\nexport const a = action(async (_a$, { id }, [first] = [], ...rest) => { try { _a$.resume(await _a$.suspend(put(id, first, rest))); } finally { _a$.end(); } });\nexport const b = action(async function (this: Window, _a$, n: number = 1) { try { _a$.resume(await _a$.suspend(wait(n))); } finally { _a$.end(); } });\nexport const c = action(function named(_a$) { try { done(); } finally { _a$.end(); } });\nexport const d = action(async <T,>(_a$, value: T): Promise<void> => { try { _a$.resume(await _a$.suspend(put(value))); } finally { _a$.end(); } });",
    ),
    (
        "nested_functions_keep_their_awaits",
        "import { $action } from \"reze-js\";\nexport const all = $action(async (list) => {\n  list.forEach((item) => { item.seen = true; });\n  await Promise.all(list.map(async (item) => { await put(item); }));\n  const inner = $action(async () => { await put(list); });\n});",
        "import { action } from \"reze-js\";\nexport const all = action(async (_a$, list) => { try {\n  list.forEach((item) => { item.seen = true; });\n  _a$.resume(await _a$.suspend(Promise.all(list.map(async (item) => { await put(item); }))));\n  const inner = action(async (_a$) => { try { _a$.resume(await _a$.suspend(put(list))); } finally { _a$.end(); } });\n} finally { _a$.end(); } });",
    ),
    (
        "signals_inside_an_action",
        "import { $action, $signal } from \"reze-js\";\nlet saving = $signal(0);\nexport const save = $action(async (t) => {\n  saving += 1;\n  t.value = await put(t, saving);\n  saving -= 1;\n});\nexport const view = <p>{saving}</p>;",
        "import { action, signal } from \"reze-js\";\nconst [saving, setSaving] = signal(0);\nexport const save = action(async (_a$, t) => { try {\n  setSaving(saving() + 1);\n  t.value = _a$.resume(await _a$.suspend(put(t, saving())));\n  setSaving(saving() - 1);\n} finally { _a$.end(); } });\nexport const view = <p>{saving()}</p>;",
    ),
    (
        "namespace_and_alias_imports",
        "import * as R from \"reze-js\";\nimport { $action as act, action } from \"reze-js\";\nexport const a = R.$action(async () => { await x(); });\nexport const b = act(() => {});\nexport const c = action((run) => run);",
        "import * as R from \"reze-js\";\nimport { action } from \"reze-js\";\nexport const a = R.action(async (_a$) => { try { _a$.resume(await _a$.suspend(x())); } finally { _a$.end(); } });\nexport const b = action((_a$) => { try {} finally { _a$.end(); } });\nexport const c = action((run) => run);",
    ),
    (
        "the_run_name_avoids_taken_names",
        "import { $action } from \"reze-js\";\nconst _a$ = 1;\nexport const a = $action(async () => { await x(_a$); });",
        "import { action } from \"reze-js\";\nconst _a$ = 1;\nexport const a = action(async (_a$2) => { try { _a$2.resume(await _a$2.suspend(x(_a$))); } finally { _a$2.end(); } });",
    ),
];

#[test]
fn compiles_to_what_a_person_writes_with_action() {
    for (name, dsl, manual) in INVARIANT {
        assert_eq!(compiled(dsl, &options()), compiled(manual, &options()), "{name}");
    }
}

#[test]
fn debug_names_name_the_declared_action() {
    let options = Options { debug_names: true, ..options() };
    let dsl = "import { $action } from \"reze-js\";\nexport const save = $action(async (t) => { await put(t); });";
    let code = compiled(dsl, &options);
    assert!(code.contains("} finally { _a$.end(); } }, { name: \"save\" });"), "{code}");
}

const SNAPSHOTS: &[(&str, &str)] = &[
    (
        "todo_toggle",
        "import { $action, For, Show, store } from \"reze-js\";\nconst todos = store([{ id: 1, done: false }]);\nconst toggle = $action(async (todo) => {\n  todo.done = !todo.done;\n  const saved = await api.toggle(todo.id);\n  todo.updatedAt = saved.updatedAt;\n});\nexport const view = (\n  <ul>\n    <For each={todos}>{(todo) => <li><input type=\"checkbox\" checked={todo().done} onChange={() => toggle(todo())} /></li>}</For>\n    <Show when={toggle.pending > 0}><p>Saving…</p></Show>\n  </ul>\n);",
    ),
    (
        "async_component_with_action",
        "import { $action } from \"reze-js\";\nexport async function Card(props) {\n  const user = await fetchUser(props.id);\n  const rename = $action(async (name) => {\n    user.name = name;\n    await saveUser(user);\n  });\n  return <button onClick={() => rename(\"x\")}>{user.name}</button>;\n}",
    ),
    (
        "sync_body_and_nested_write_warning",
        "import { $action } from \"reze-js\";\nexport const bump = $action((state) => {\n  state.n += 1;\n});\nexport const save = $action(async (todo) => {\n  api.save(todo).then((saved) => {\n    todo.at = saved.at;\n  });\n  setTimeout(() => { delete todo.draft; });\n  const later = async () => { todo.count++; };\n  await later();\n});",
    ),
];

fn render(source: &str) -> String {
    let out = match compile(source, "case.tsx", &options()) {
        Ok(Some(out)) => out,
        Ok(None) => return "<unchanged>".to_string(),
        Err(errors) => {
            return errors.iter().map(|d| d.rendered.clone()).collect::<Vec<_>>().join("\n\n");
        }
    };
    let mut text = out.code;
    for diagnostic in &out.diagnostics {
        text.push_str("\n// ");
        text.push_str(diagnostic.severity.as_str());
        text.push('\n');
        text.push_str(&diagnostic.rendered);
    }
    text
}

#[test]
fn output_snapshots() {
    for (name, source) in SNAPSHOTS {
        insta::assert_snapshot!(*name, render(source), source);
    }
}

fn module(body: &str) -> String {
    format!("import {{ $action }} from \"reze-js\";\n{body}")
}

#[test]
fn an_argument_that_is_not_a_function_literal_is_refused() {
    for argument in ["save", "make()", "", "...handlers"] {
        let source = module(&format!("export const a = $action({argument});"));
        let d = only(&source, Code::ActionArgument);
        assert_eq!(d.severity, Severity::Error);
    }
    let d = only(&module("export const a = $action(save);"), Code::ActionArgument);
    assert_eq!((d.start.line, d.start.column, d.end.column), (2, 25, 29));
}

#[test]
fn suspensions_the_compiler_cannot_resume_are_refused_but_nested_ones_are_not() {
    for (body, construct) in [
        ("async (s) => { for await (const x of s) use(x); }", "for await"),
        ("async () => { await using r = open(); }", "await using"),
        ("function* () { yield 1; }", "generator"),
        ("async function* () {}", "generator"),
    ] {
        let d =
            only(&module(&format!("export const a = $action({body});")), Code::ActionUnsupported);
        assert_eq!(d.data["construct"], construct, "{body}");
    }
    let nested = module(
        "export const a = $action(async (s) => { await drain(async () => { for await (const x of s) use(x); }); });",
    );
    assert!(compiled(&nested, &options()).contains("for await (const x of s)"));
}

#[test]
fn syntax_used_as_a_value_is_not_called() {
    only(&module("export const make = $action;"), Code::ActionNotCalled);
    only(&module("register($action);"), Code::ActionNotCalled);
    only("import * as R from \"reze-js\";\nexport const make = R.$action;", Code::ActionNotCalled);
}

#[test]
fn a_member_write_in_a_function_that_runs_later_warns_and_one_in_a_sync_callback_does_not() {
    for (nested, via) in [
        ("p.then((v) => { t.v = v; });", "a `.then` callback"),
        ("p.catch(function () { t.failed = true; });", "a `.catch` callback"),
        ("setTimeout(() => t.n++);", "a `setTimeout` callback"),
        ("queueMicrotask(() => { delete t.draft; });", "a `queueMicrotask` callback"),
        ("list.map(async (x) => { x.saved = true; });", "an async function"),
        ("p.then(() => { list.forEach((x) => { x.v = 1; }); });", "a `.then` callback"),
    ] {
        let source =
            module(&format!("export const a = $action(async (t, p, list) => {{ {nested} }});"));
        let d = only(&source, Code::ActionNestedWrite);
        assert_eq!((d.severity, d.data["via"].as_str()), (Severity::Warn, via), "{nested}");
    }
    for fine in [
        "list.forEach((x) => { x.v = 1; });",
        "t.v = await p;",
        "let n = 0; setTimeout(() => { n = 1; });",
        "const inner = $action(async () => { await p; t.v = 1; });",
    ] {
        let source =
            module(&format!("export const a = $action(async (t, p, list) => {{ {fine} }});"));
        let (_, diagnostics) = output(&source, &options());
        assert!(diagnostics.iter().all(|d| d.code != Code::ActionNestedWrite), "{fine}");
    }
    let outside = module("p.then((v) => { t.v = v; });\nexport const a = $action(() => {});");
    let (_, diagnostics) = output(&outside, &options());
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}
