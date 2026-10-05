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
    let at = "import { $signal } from \"reze-js\";\nexport const view = <p>{$signal(0)}</p>;";
    let d = only(at, Code::SignalNotDeclared);
    assert_eq!((d.start.line, d.start.column), (2, 24));
    only("import { $signal } from \"reze-js\";\nvar a = $signal(0);", Code::SignalNotDeclared);
    only(
        "import { $signal } from \"reze-js\";\nconst make = $signal;\nmake(0);",
        Code::SignalNotDeclared,
    );
    only(
        "import * as R from \"reze-js\";\nfunction f() { return R.$signal(0); }",
        Code::SignalNotDeclared,
    );
}

#[test]
fn a_destructured_declaration_is_a_pattern() {
    let d =
        only("import { $signal } from \"reze-js\";\nlet [a] = $signal([0]);", Code::SignalPattern);
    assert_eq!((d.start.line, d.start.column), (2, 4));
}

#[test]
fn an_exported_signal_is_refused_however_it_is_exported() {
    for export in [
        "export let a = $signal(0);",
        "let a = $signal(0);\nexport { a };",
        "let a = $signal(0);\nexport default a;",
    ] {
        let source = format!("import {{ $signal }} from \"reze-js\";\n{export}");
        only(&source, Code::SignalExported);
    }
}

#[test]
fn a_write_the_language_performs_is_an_assign_pattern() {
    for write in ["[a] = list;", "({ a } = obj);", "for (a of list) {}", "for (a in obj) {}"] {
        let source = format!(
            "import {{ $signal }} from \"reze-js\";\nlet a = $signal(0);\nexport function f(list, obj) {{\n  {write}\n}}"
        );
        only(&source, Code::SignalAssignPattern);
    }
}

#[test]
fn an_update_inside_an_expression_is_refused_and_a_statement_is_not() {
    for update in ["use(a++);", "const b = --a;", "run(() => a++);", "const b = a++ + 1;"] {
        let source = format!(
            "import {{ $signal }} from \"reze-js\";\nlet a = $signal(0);\nexport function f() {{\n  {update}\n}}"
        );
        only(&source, Code::SignalUpdateInExpression);
    }
    let fine = "import { $signal } from \"reze-js\";\nlet a = $signal(0);\nexport function f(ok) {\n  a++;\n  ok && a--;\n  for (let i = 0; i < 2; i++, a++) {}\n}\nexport const v = <p>{a}</p>;";
    compiled(fine, &options());
}

#[test]
fn first_pass_errors_replace_the_output() {
    let source = "import { $signal } from \"reze-js\";\nlet [a] = $signal(0);\nexport const view = <p>{a}</p>;";
    assert!(errors(source).iter().all(|d| d.severity == Severity::Error));
}

#[test]
fn reading_once_is_reported_only_for_top_level_initializers_of_a_component() {
    let source = "import { $signal } from \"reze-js\";\nimport { computed } from \"@rezejs/signals\";\nlet count = $signal(0);\nexport function Counter() {\n  const copy = count;\n  const live = computed(() => count * 2);\n  const view = <b>{count}</b>;\n  const seeded = $signal(count);\n  const handler = () => { const inner = count; };\n  return view;\n}\nfunction helper() {\n  const plain = count;\n  return plain;\n}";
    let out = compile(source, "test.tsx", &options()).unwrap().unwrap();
    let once: Vec<_> = out.diagnostics.iter().filter(|d| d.code == Code::SignalReadOnce).collect();
    assert_eq!(once.len(), 1, "{once:?}");
    assert_eq!(once[0].start.line, 5);
    assert_eq!(once[0].severity, Severity::Warn);
    assert_eq!(once[0].data["variable"], "copy");
}

#[test]
fn diagnostics_of_the_second_pass_point_into_the_original_source() {
    let source = "import { $signal, For } from \"reze-js\";\n\nlet selected = $signal(0);\nexport const view = (\n  <For each={rows()}>{(row) => <li class={selected === row.id ? \"on\" : \"\"} onClick={() => selected = row.id} />}</For>\n);";
    let out = compile(source, "test.tsx", &options()).unwrap().unwrap();
    let auto = out.diagnostics.iter().find(|d| d.code == Code::AutoSelector).expect("selector");
    let line = source.lines().nth(auto.start.line as usize - 1).unwrap();
    assert_eq!(
        &line[auto.start.column as usize..],
        "selected === row.id ? \"on\" : \"\"} onClick={() => selected = row.id} />}</For>"
    );
    assert_eq!(auto.data["signal"], "selected");

    let folded = "import { $signal } from \"reze-js\";\nlet title = $signal(\"x\");\nexport const v = <p>{title}</p>;";
    let out = compile(folded, "test.tsx", &options()).unwrap().unwrap();
    let info = out.diagnostics.iter().find(|d| d.code == Code::SignalFolded).expect("folded");
    assert_eq!((info.start.line, info.start.column), (2, 4));
    assert_eq!((info.end.line, info.end.column), (2, 24));
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
    let source = "import { $signal } from \"reze-js\";\n\nlet count = $signal(0);\nexport function bump() {\n  count += 1;\n  return count;\n}\nexport const view = <p>{count}</p>;";
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
    let source = "import { $signal } from \"reze-js\";\nlet count = $signal(0);\nexport const read = () => count;\nexport const inc = () => { count++; };";
    let out = compile(source, "test.ts", &Options::default()).unwrap().unwrap();
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &out.code, SourceType::ts()).parse();
    assert!(parsed.diagnostics.is_empty(), "{:?}\n{}", parsed.diagnostics, out.code);
    assert!(!out.code.contains("$signal"), "{}", out.code);
    assert!(out.code.contains("count()"), "{}", out.code);
    let json = out.map.expect("map");
    let map = SourceMap::from_json_string(&json).unwrap();
    let (line, _) = mapped_position(&out.code, &map, "export const read");
    assert_eq!(line, 2);
}

#[test]
fn a_file_that_mentions_the_name_without_importing_it_is_left_alone() {
    assert!(
        compile("const $signal = 1; export const a = $signal;", "a.ts", &options())
            .unwrap()
            .is_none()
    );
}

fn computed_module(body: &str) -> String {
    format!(
        "import {{ $computed, $signal }} from \"reze-js\";\nlet count = $signal(0);\nconst d = $computed(count * 2);\n{body}"
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
    let source = "import { $computed, $signal } from \"reze-js\";\nlet count = $signal(0);\nconst d = $computed(() => ({ n: count }));\nexport const v = <p onClick={() => { count++; }}>{d.n}</p>;";
    let d = only(source, Code::ComputedFunction);
    let edit = &d.fixes[0].edits[0];
    let mut fixed = source.to_string();
    fixed.replace_range(edit.start as usize..edit.end as usize, &edit.text);
    assert!(fixed.contains("$computed(({ n: count }))"), "{fixed}");
    assert!(compiled(&fixed, &options()).contains("count()"), "{fixed}");

    for literal in [
        "async () => count",
        "(x) => count",
        "() => { return count; }",
        "function () { return count; }",
    ] {
        let source = computed_module(&format!(
            "const e = $computed({literal});\nexport const v = <p>{{e}}</p>;"
        ));
        assert!(only(&source, Code::ComputedFunction).fixes.is_empty(), "{literal}");
    }
}

#[test]
fn awaiting_inside_the_expression_is_refused_but_inside_a_nested_function_is_not() {
    let source = "import { $computed } from \"reze-js\";\nexport async function load(id) {\n  const d = $computed(await fetchUser(id));\n  return d;\n}";
    only(source, Code::ComputedAwait);
    let nested = "import { $computed } from \"reze-js\";\nexport function load(id) {\n  const d = $computed(pick(async () => await fetchUser(id)));\n  return d;\n}";
    let code = compiled(nested, &options());
    assert!(code.contains("await fetchUser"), "{code}");
}

#[test]
fn computed_declaration_errors_name_the_primitive() {
    let d = only(
        "import { $computed } from \"reze-js\";\nexport const view = <p>{$computed(1)}</p>;",
        Code::SignalNotDeclared,
    );
    assert_eq!(d.data["primitive"], "$computed");
    assert!(d.message.contains("`$computed` is only valid"), "{}", d.message);
    only("import { $computed } from \"reze-js\";\nvar a = $computed(1);", Code::SignalNotDeclared);
    let d = only(
        "import { $computed } from \"reze-js\";\nconst { a } = $computed({ a: 1 });",
        Code::SignalPattern,
    );
    assert_eq!(d.data["primitive"], "$computed");
    let d = only(&computed_module("export { d };"), Code::SignalExported);
    assert_eq!((d.data["signal"].as_str(), d.data["primitive"].as_str()), ("d", "$computed"));
}

#[test]
fn reading_a_computed_once_warns_and_declaring_one_does_not() {
    let source = "import { $computed, $signal, untrack } from \"reze-js\";\nlet count = $signal(0);\nexport function Counter() {\n  const doubled = $computed(count * 2);\n  const copy = doubled;\n  const seed = untrack(() => doubled);\n  return <p onClick={() => { count++; }}>{copy}{seed}</p>;\n}";
    let out = compile(source, "test.tsx", &options()).unwrap().unwrap();
    let once: Vec<_> = out.diagnostics.iter().filter(|d| d.code == Code::SignalReadOnce).collect();
    assert_eq!(once.len(), 1, "{once:?}");
    assert_eq!((once[0].start.line, once[0].data["signal"].as_str()), (5, "doubled"));
}
