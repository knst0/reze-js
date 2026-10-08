use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_sourcemap::SourceMap;
use oxc_span::SourceType;
use reze_compiler::{Code, Diagnostic, Options, Severity, compile};

fn options() -> Options {
    Options { source_map: false, ..Options::default() }
}

fn compiled(source: &str, options: &Options) -> String {
    let code = match compile(source, "test.tsx", options).expect("compiles") {
        Some(out) => out.code,
        None => source.to_string(),
    };
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &code, SourceType::tsx()).parse();
    assert!(parsed.diagnostics.is_empty(), "{:?}\n{code}", parsed.diagnostics);
    let semantic = SemanticBuilder::new().with_check_syntax_error(true).build(&parsed.program);
    assert!(semantic.diagnostics.is_empty(), "{:?}\n{code}", semantic.diagnostics);
    code
}

fn errors(source: &str) -> Vec<Diagnostic> {
    compile(source, "test.tsx", &options()).err().expect("fails")
}

fn only(source: &str, code: Code) -> Diagnostic {
    let found: Vec<_> = errors(source).into_iter().filter(|d| d.code == code).collect();
    assert_eq!(found.len(), 1, "{found:?}");
    found.into_iter().next().unwrap()
}

#[test]
fn a_call_outside_a_declaration_is_not_declared() {
    let at = "import { signal } from \"reze-js\";\nexport const view = <p>{signal(0)}</p>;";
    let d = only(at, Code::SignalNotDeclared);
    assert_eq!((d.start.line, d.start.column), (2, 24));
    only("import { signal } from \"reze-js\";\nvar a = signal(0);", Code::SignalNotDeclared);
    only(
        "import { signal } from \"reze-js\";\nconst make = signal;\nmake(0);",
        Code::SignalNotDeclared,
    );
    only(
        "import * as R from \"reze-js\";\nfunction f() { return R.signal(0); }",
        Code::SignalNotDeclared,
    );
}

#[test]
fn a_destructured_declaration_is_a_pattern() {
    let d =
        only("import { signal } from \"reze-js\";\nlet [a] = signal([0]);", Code::SignalPattern);
    assert_eq!((d.start.line, d.start.column), (2, 4));
}

fn fixed(source: &str) -> Option<String> {
    let d = only(source, Code::SignalPattern);
    let fix = d.fixes.first()?;
    let mut edits: Vec<_> = fix.edits.iter().collect();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.start));
    let mut text = source.to_string();
    for edit in edits {
        text.replace_range(edit.start as usize..edit.end as usize, &edit.text);
    }
    Some(text)
}

#[test]
fn a_tuple_declaration_is_rewritten_to_one_variable_by_its_fix() {
    let source = "import { signal } from \"reze-js\";\nconst [n, setN] = signal(0);\nexport const view = <button onClick={() => setN(n() + 1)}>{n()}</button>;";
    assert_eq!(
        fixed(source).as_deref(),
        Some(
            "import { signal } from \"reze-js\";\nlet n = signal(0);\nexport const view = <button onClick={() => n = n + 1}>{n}</button>;"
        )
    );
}

#[test]
fn a_tuple_whose_setter_escapes_has_no_fix() {
    for source in [
        "import { signal } from \"reze-js\";\nconst [n, setN] = signal(0);\nexport const f = () => register(setN);",
        "import { signal } from \"reze-js\";\nconst [n, setN] = signal(0);\nexport const f = () => setN((v) => v + 1);",
        "import { signal } from \"reze-js\";\nconst [n] = signal(0);\nexport const f = () => register(n);",
    ] {
        assert_eq!(fixed(source), None, "{source}");
    }
}

fn exported_names(code: &str) -> Vec<String> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, SourceType::tsx()).parse();
    parsed
        .program
        .body
        .iter()
        .flat_map(|statement| match statement {
            oxc_ast::ast::Statement::ExportNamedDeclaration(export) => export
                .specifiers
                .iter()
                .map(|specifier| specifier.exported.name().to_string())
                .collect::<Vec<_>>(),
            oxc_ast::ast::Statement::ExportDeclaration(export) => match &export.declaration {
                oxc_ast::ast::Declaration::VariableDeclaration(variables) => variables
                    .declarations
                    .iter()
                    .flat_map(|declarator| declarator.id.get_binding_identifiers())
                    .map(|id| id.name.to_string())
                    .collect(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn a_named_export_of_a_signal_exports_the_getter_and_keeps_the_setter_private() {
    for (body, expected) in [
        (
            "export let count = signal(0);\nexport const bump = () => (count += 1);",
            ["count", "bump"],
        ),
        (
            "let count = signal(0);\nexport { count };\nexport const bump = () => (count += 1);",
            ["count", "bump"],
        ),
        (
            "let count = signal(0);\nexport { count as value };\nexport const bump = () => (count += 1);",
            ["value", "bump"],
        ),
    ] {
        let code = compiled(&format!("import {{ signal }} from \"reze-js\";\n{body}"), &options());
        assert_eq!(exported_names(&code), expected, "{code}");
    }
}

#[test]
fn a_reactive_binding_cannot_be_the_default_export() {
    for export in ["export default count;", "export { count as default };"] {
        let source =
            format!("import {{ signal }} from \"reze-js\";\nlet count = signal(0);\n{export}");
        let d = only(&source, Code::SignalDefaultExport);
        assert_eq!((d.data["signal"].as_str(), d.data["primitive"].as_str()), ("count", "signal"));
    }
}

#[test]
fn a_write_the_language_performs_is_an_assign_pattern() {
    for write in ["[a] = list;", "({ a } = obj);", "for (a of list) {}", "for (a in obj) {}"] {
        let source = format!(
            "import {{ signal }} from \"reze-js\";\nlet a = signal(0);\nexport function f(list, obj) {{\n  {write}\n}}"
        );
        only(&source, Code::SignalAssignPattern);
    }
}

#[test]
fn an_update_inside_an_expression_is_refused_and_a_statement_is_not() {
    for update in ["use(a++);", "const b = --a;", "run(() => a++);", "const b = a++ + 1;"] {
        let source = format!(
            "import {{ signal }} from \"reze-js\";\nlet a = signal(0);\nexport function f() {{\n  {update}\n}}"
        );
        only(&source, Code::SignalUpdateInExpression);
    }
    let fine = "import { signal } from \"reze-js\";\nlet a = signal(0);\nexport function f(ok) {\n  a++;\n  ok && a--;\n  for (let i = 0; i < 2; i++, a++) {}\n}\nexport const v = <p>{a}</p>;";
    compiled(fine, &options());
}

#[test]
fn first_pass_errors_replace_the_output() {
    let source = "import { signal } from \"reze-js\";\nlet [a] = signal(0);\nexport const view = <p>{a}</p>;";
    assert!(errors(source).iter().all(|d| d.severity == Severity::Error));
}

#[test]
fn reading_once_is_reported_only_for_top_level_initializers_of_a_component() {
    let source = "import { computed, signal } from \"reze-js\";\nlet count = signal(0);\nexport function Counter() {\n  const copy = count;\n  const live = computed(count * 2);\n  const view = <b>{count}</b>;\n  const seeded = signal(count);\n  const handler = () => { const inner = count; };\n  return view;\n}\nfunction helper() {\n  const plain = count;\n  return plain;\n}";
    let out = compile(source, "test.tsx", &options()).unwrap().unwrap();
    let once: Vec<_> = out.diagnostics.iter().filter(|d| d.code == Code::SignalReadOnce).collect();
    assert_eq!(once.len(), 1, "{once:?}");
    assert_eq!(once[0].start.line, 4);
    assert_eq!(once[0].severity, Severity::Warn);
    assert_eq!(once[0].data["variable"], "copy");
}

#[test]
fn diagnostics_of_the_second_pass_point_into_the_original_source() {
    let source = "import { signal, For } from \"reze-js\";\n\nlet selected = signal(0);\nexport const view = (\n  <For each={rows()}>{(row) => <li class={selected === row.id ? \"on\" : \"\"} onClick={() => selected = row.id} />}</For>\n);";
    let out = compile(source, "test.tsx", &options()).unwrap().unwrap();
    let auto = out.diagnostics.iter().find(|d| d.code == Code::AutoSelector).expect("selector");
    let line = source.lines().nth(auto.start.line as usize - 1).unwrap();
    assert_eq!(
        &line[auto.start.column as usize..],
        "selected === row.id ? \"on\" : \"\"} onClick={() => selected = row.id} />}</For>"
    );
    assert_eq!(auto.data["signal"], "selected");

    let folded = "import { signal } from \"reze-js\";\nlet title = signal(\"x\");\nexport const v = <p>{title}</p>;";
    let out = compile(folded, "test.tsx", &options()).unwrap().unwrap();
    let info = out.diagnostics.iter().find(|d| d.code == Code::SignalFolded).expect("folded");
    assert_eq!((info.start.line, info.start.column), (2, 4));
    assert_eq!((info.end.line, info.end.column), (2, 23));
}

fn mapped_position(out_code: &str, map: &SourceMap, needle: &str) -> (u32, u32) {
    let offset = out_code.find(needle).expect(needle);
    let line = out_code[..offset].matches('\n').count() as u32;
    let column = offset - out_code[..offset].rfind('\n').map_or(0, |i| i + 1);
    let table = map.generate_lookup_table();
    let token = map.lookup_token(&table, line, column as u32).expect("token");
    (token.get_src_line(), token.get_src_col())
}

#[test]
fn the_source_map_points_at_the_original_source() {
    let source = "import { signal } from \"reze-js\";\n\nlet count = signal(0);\nexport function bump() {\n  count += 1;\n  return count;\n}\nexport const view = <p>{count}</p>;";
    let out = compile(source, "test.tsx", &Options::default()).unwrap().unwrap();
    let json = out.map.expect("map");
    let map = SourceMap::from_json_string(&json).unwrap();
    assert_eq!(map.get_source_content(0), Some(source));
    let (line, _) = mapped_position(&out.code, &map, "return count()");
    assert_eq!(line, 5);
    let (line, _) = mapped_position(&out.code, &map, "setCount(");
    assert_eq!(line, 4);
    let (line, _) = mapped_position(&out.code, &map, "export const view");
    assert_eq!(line, 7);
    let (line, _) = mapped_position(&out.code, &map, "const [count, setCount]");
    assert_eq!(line, 2);
}

#[test]
fn a_module_without_jsx_still_comes_out_rewritten_with_a_map() {
    let source = "import { signal } from \"reze-js\";\nlet count = signal(0);\nexport const read = () => count;\nexport const inc = () => { count++; };";
    let out = compile(source, "test.ts", &Options::default()).unwrap().unwrap();
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &out.code, SourceType::ts()).parse();
    assert!(parsed.diagnostics.is_empty(), "{:?}\n{}", parsed.diagnostics, out.code);
    assert!(!out.code.contains("from \"reze-js\""), "{}", out.code);
    assert!(out.code.contains("count()"), "{}", out.code);
    assert!(out.code.contains("reze-js/internal/reactivity"), "{}", out.code);
    let json = out.map.expect("map");
    let map = SourceMap::from_json_string(&json).unwrap();
    let (line, _) = mapped_position(&out.code, &map, "export const read");
    assert_eq!(line, 2);
}

#[test]
fn a_file_that_mentions_the_name_without_importing_it_is_left_alone() {
    assert!(
        compile("const signal = 1; export const a = signal;", "a.ts", &options())
            .unwrap()
            .is_none()
    );
}

fn computed_module(body: &str) -> String {
    format!(
        "import {{ computed, signal }} from \"reze-js\";\nlet count = signal(0);\nconst d = computed(count * 2);\n{body}"
    )
}

#[test]
fn every_write_to_a_computed_is_refused() {
    for write in [
        "export const f = () => { d = 1; };",
        "export const f = () => { d += 1; };",
        "export const f = () => { d ||= 1; };",
        "export const f = () => { d++; };",
        "export const f = (list) => { [d] = list; };",
        "export const f = (list) => { for (d of list) {} };",
    ] {
        let d = only(&computed_module(write), Code::ComputedWritten);
        assert_eq!(d.data["computed"], "d", "{write}");
    }
}

#[test]
fn a_function_literal_argument_is_refused_and_the_fix_unwraps_it() {
    let source = "import { computed, signal } from \"reze-js\";\nlet count = signal(0);\nconst d = computed(() => ({ n: count }));\nexport const v = <p onClick={() => { count++; }}>{d.n}</p>;";
    let d = only(source, Code::ComputedFunction);
    let edit = &d.fixes[0].edits[0];
    let mut fixed = source.to_string();
    fixed.replace_range(edit.start as usize..edit.end as usize, &edit.text);
    assert!(fixed.contains("computed(({ n: count }))"), "{fixed}");
    assert!(compiled(&fixed, &options()).contains("count()"), "{fixed}");

    for literal in [
        "async () => count",
        "(x) => count",
        "() => { return count; }",
        "function () { return count; }",
    ] {
        let source = computed_module(&format!(
            "const e = computed({literal});\nexport const v = <p>{{e}}</p>;"
        ));
        assert!(only(&source, Code::ComputedFunction).fixes.is_empty(), "{literal}");
    }
}

#[test]
fn awaiting_inside_computed_lowers_to_a_resource_and_leaves_the_component_unsuspended() {
    let source = "import { computed } from \"reze-js\";\nexport async function Card(props) {\n  const id = props.id;\n  const user = computed(await fetchUser(id));\n  return <p>{user}</p>;\n}";
    let code = compiled(source, &options());
    assert!(code.contains("resource("), "{code}");
    assert!(code.contains("async () =>"), "{code}");
    assert!(!code.contains("asyncComponent("), "{code}");
}

#[test]
fn resource_only_component_takes_its_views_into_the_return() {
    let source = "import { computed } from \"reze-js\";\nasync function Card(props) {\n  const user = computed(await fetchUser(props.id));\n  return <p>{user}</p>;\n}\nCard.pending = <p>loading</p>;";
    let code = compiled(source, &options());
    assert!(code.contains("asyncViews("), "{code}");
    assert!(!code.contains("Card.pending ="), "{code}");
}

#[test]
fn a_view_assigned_inside_a_nested_scope_is_taken_into_its_component() {
    let source = "export function mount() {\n  async function Card(props) {\n    const id = props.id;\n    const user = await fetchUser(id);\n    return <p>{user}</p>;\n  }\n  Card.failure = (error) => <p>{String(error)}</p>;\n  return Card;\n}";
    let code = compiled(source, &options());
    assert!(code.contains("asyncViews("), "{code}");
    assert!(!code.contains("Card.failure ="), "{code}");
}

#[test]
fn a_view_on_a_name_that_is_not_an_async_component_here_is_refused() {
    let d = only(
        "function Card(props) {\n  return <p>{props.id}</p>;\n}\nCard.failure = (error) => <p>{String(error)}</p>;",
        Code::AsyncViewTarget,
    );
    assert_eq!(d.data["name"], "Card");
    assert_eq!(d.data["view"], "failure");
}

#[test]
fn a_lowercase_object_with_a_pending_property_is_not_a_view() {
    let source = "export function mount(state) {\n  state.pending = true;\n  return state;\n}";
    let reported = match compile(source, "test.tsx", &options()) {
        Ok(Some(out)) => out.diagnostics,
        Ok(None) => Vec::new(),
        Err(diagnostics) => diagnostics,
    };
    assert!(reported.iter().all(|d| d.code != Code::AsyncViewTarget), "{reported:?}");
}

#[test]
fn computed_declaration_errors_name_the_primitive() {
    let d = only(
        "import { computed } from \"reze-js\";\nexport const view = <p>{computed(1)}</p>;",
        Code::SignalNotDeclared,
    );
    assert_eq!(d.data["primitive"], "computed");
    assert!(d.message.contains("`computed` is only valid"), "{}", d.message);
    only("import { computed } from \"reze-js\";\nvar a = computed(1);", Code::SignalNotDeclared);
    let d = only(
        "import { computed } from \"reze-js\";\nconst { a } = computed({ a: 1 });",
        Code::SignalPattern,
    );
    assert_eq!(d.data["primitive"], "computed");
    let d = only(&computed_module("export default d;"), Code::SignalDefaultExport);
    assert_eq!((d.data["signal"].as_str(), d.data["primitive"].as_str()), ("d", "computed"));
}

#[test]
fn reading_a_computed_once_warns_and_declaring_one_does_not() {
    let source = "import { computed, signal, untrack } from \"reze-js\";\nlet count = signal(0);\nexport function Counter() {\n  const doubled = computed(count * 2);\n  const copy = doubled;\n  const seed = untrack(() => doubled);\n  return <p onClick={() => { count++; }}>{copy}{seed}</p>;\n}";
    let out = compile(source, "test.tsx", &options()).unwrap().unwrap();
    let once: Vec<_> = out.diagnostics.iter().filter(|d| d.code == Code::SignalReadOnce).collect();
    assert_eq!(once.len(), 1, "{once:?}");
    assert_eq!((once[0].start.line, once[0].data["signal"].as_str()), (5, "doubled"));
}
