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
    let names_syntax = code.contains("$signal")
        || code.match_indices("$computed").any(|(at, _)| !code[..at].ends_with('_'));
    assert!(!names_syntax, "{code}");
    code
}

fn errors(source: &str) -> Vec<Diagnostic> {
    compile(source, "test.tsx", &options()).err().expect("fails")
}

/// Each pair is a `$signal` module and what a person writes with `signal`; both must compile to
/// the same text.
const INVARIANT: &[(&str, &str, &str)] = &[
    (
        "reads_and_updates",
        "import { $signal } from \"reze-js\";\nlet count = $signal(0);\nexport const view = <button class={count > 1 ? \"big\" : \"\"} onClick={() => { count++; count += 2; }}>{count}</button>;",
        "import { signal } from \"reze-js\";\nconst [count, setCount] = signal(0);\nexport const view = <button class={count() > 1 ? \"big\" : \"\"} onClick={() => { setCount(count() + 1); setCount(count() + 2); }}>{count()}</button>;",
    ),
    (
        "options_shorthand_and_function_values",
        "import { $signal } from \"reze-js\";\nlet name = $signal(\"a\", { equals: false });\nlet handler = $signal<() => void>();\nexport function save() {\n  handler = () => name;\n  return { name };\n}",
        "import { signal } from \"reze-js\";\nconst [name, setName] = signal(\"a\", { equals: false });\nconst [handler, setHandler] = signal<() => void>();\nexport function save() {\n  setHandler(() => () => name());\n  return { name: name() };\n}",
    ),
    (
        "assignment_values",
        "import { $signal } from \"reze-js\";\nlet c = $signal(0);\nexport function f(a, b, list) {\n  c = 1;\n  c = `x${a}`;\n  c = { a };\n  c = [a];\n  c = a + b;\n  c = -a;\n  c = a ? 1 : 2;\n  c = a ? b : 2;\n  c = pick(list);\n  c = c = 3;\n}\nexport const v = <p>{c}</p>;",
        "import { signal } from \"reze-js\";\nconst [c, setC] = signal(0);\nexport function f(a, b, list) {\n  setC(1);\n  setC(`x${a}`);\n  setC({ a });\n  setC([a]);\n  setC(a + b);\n  setC(-a);\n  setC(a ? 1 : 2);\n  setC(() => a ? b : 2);\n  setC(() => pick(list));\n  setC(setC(3));\n}\nexport const v = <p>{c()}</p>;",
    ),
    (
        "compound_operators",
        "import { $signal } from \"reze-js\";\nlet n = $signal(1);\nexport function f(a, b) {\n  n -= a;\n  n *= a + b;\n  n **= 2;\n  n--;\n  --n;\n  n %= -a;\n  n <<= pick(a);\n}\nexport const v = <p>{n}</p>;",
        "import { signal } from \"reze-js\";\nconst [n, setN] = signal(1);\nexport function f(a, b) {\n  setN(n() - a);\n  setN(n() * (a + b));\n  setN(n() ** 2);\n  setN(n() - 1);\n  setN(n() - 1);\n  setN(n() % -a);\n  setN(n() << pick(a));\n}\nexport const v = <p>{n()}</p>;",
    ),
    (
        "logical_assignment",
        "import { $signal } from \"reze-js\";\nlet c = $signal(0);\nexport function f(a) {\n  c ||= 4;\n  c &&= a;\n  c ??= () => a;\n  const x = (c ??= 2);\n  return a && (c ||= 1);\n}\nexport const v = <p>{c}</p>;",
        "import { signal } from \"reze-js\";\nconst [c, setC] = signal(0);\nexport function f(a) {\n  c() || setC(4);\n  c() && setC(() => a);\n  c() ?? setC(() => () => a);\n  const x = ((c() ?? setC(2)));\n  return a && ((c() || setC(1)));\n}\nexport const v = <p>{c()}</p>;",
    ),
    (
        "update_positions",
        "import { $signal } from \"reze-js\";\nlet i = $signal(0);\nexport function f(ok) {\n  for (i = 0; i < 3; i++) {}\n  ok && i++;\n  ok ? i++ : i--;\n  i++, i--;\n  void i++;\n}\nexport const v = <p>{i}</p>;",
        "import { signal } from \"reze-js\";\nconst [i, setI] = signal(0);\nexport function f(ok) {\n  for (setI(0); i() < 3; setI(i() + 1)) {}\n  ok && setI(i() + 1);\n  ok ? setI(i() + 1) : setI(i() - 1);\n  setI(i() + 1), setI(i() - 1);\n  void setI(i() + 1);\n}\nexport const v = <p>{i()}</p>;",
    ),
    (
        "namespace_import",
        "import * as R from \"reze-js\";\nlet n = R.$signal(0);\nexport const v = <p>{n}</p>;",
        "import * as R from \"reze-js\";\nconst [n] = R.signal(0);\nexport const v = <p>{n()}</p>;",
    ),
    (
        "merged_with_signal_import",
        "import { $signal, computed, signal } from \"reze-js\";\nlet a = $signal(0);\nconst [b, setB] = signal(1);\nconst sum = computed(() => a + b());\nexport const v = <p onClick={() => setB(2)}>{sum()}</p>;",
        "import { computed, signal } from \"reze-js\";\nconst [a] = signal(0);\nconst [b, setB] = signal(1);\nconst sum = computed(() => a() + b());\nexport const v = <p onClick={() => setB(2)}>{sum()}</p>;",
    ),
    (
        "aliased_import",
        "import { $signal as sig } from \"reze-js\";\nlet c = sig(0);\nexport const v = <p onClick={() => c += 1}>{c}</p>;",
        "import { signal as sig } from \"reze-js\";\nconst [c, setC] = sig(0);\nexport const v = <p onClick={() => setC(c() + 1)}>{c()}</p>;",
    ),
    (
        "sole_import_dropped",
        "import { $signal } from \"reze-js\";\nimport { signal } from \"@rezejs/signals\";\nlet a = $signal(0);\nexport const v = <p onClick={() => a = 1}>{a}</p>;",
        "import { signal } from \"@rezejs/signals\";\nconst [a, setA] = signal(0);\nexport const v = <p onClick={() => setA(1)}>{a()}</p>;",
    ),
    (
        "declarations_keep_their_neighbours",
        "import { $signal } from \"reze-js\";\nlet a = $signal(0), other = 1;\nother = 2;\nfor (let i = $signal(0); i < 2; i++) {}\nexport const v = <p onClick={() => a = other}>{a}</p>;",
        "import { signal } from \"reze-js\";\nlet [a, setA] = signal(0), other = 1;\nother = 2;\nfor (let [i, setI] = signal(0); i() < 2; setI(i() + 1)) {}\nexport const v = <p onClick={() => setA(() => other)}>{a()}</p>;",
    ),
];

#[test]
fn compiles_to_what_a_person_writes_with_signal() {
    for (name, dsl, manual) in INVARIANT {
        assert_eq!(compiled(dsl, &options()), compiled(manual, &options()), "{name}");
    }
}

#[test]
fn debug_names_name_the_declared_variable() {
    let options = Options { debug_names: true, ..options() };
    let dsl = "import { $signal } from \"reze-js\";\nlet count = $signal(0);\nexport const view = <button onClick={() => count += 1}>{count}</button>;";
    let manual = "import { signal } from \"reze-js\";\nconst [count, setCount] = signal(0);\nexport const view = <button onClick={() => setCount(count() + 1)}>{count()}</button>;";
    let code = compiled(dsl, &options);
    assert!(code.contains("{ name: \"count\" }"), "{code}");
    assert_eq!(code, compiled(manual, &options));
}

const SNAPSHOTS: &[(&str, &str)] = &[
    (
        "counter",
        "import { $signal } from \"reze-js\";\nlet count = $signal(0);\nconst label = $signal(\"clicks\");\nexport const view = (\n  <button onClick={() => count += 1}>{label}: {count}</button>\n);",
    ),
    (
        "closures_and_effects",
        "import { $signal, effect } from \"reze-js\";\nlet count = $signal(0);\neffect(() => console.log(count, { count }, [count]));\nexport const inc = () => { count++; };\nexport function reset() {\n  const log = () => count;\n  count = 0;\n  return log;\n}",
    ),
    (
        "jsx_attributes_and_children",
        "import { $signal } from \"reze-js\";\nlet size = $signal(1);\nlet text = $signal(\"\");\nexport const view = (\n  <div class={size > 1 ? \"big\" : \"\"} style={{ width: size * 10 + \"px\" }}>\n    <input value={text} onInput={(e) => text = e.currentTarget.value} />\n    <Card size={size}>{size} {text}</Card>\n  </div>\n);",
    ),
    (
        "show_and_for",
        "import { $signal, For, Show } from \"reze-js\";\nlet rows = $signal([{ id: 1 }, { id: 2 }]);\nlet selected = $signal(0);\nlet open = $signal(false);\nexport const view = (\n  <ul>\n    <For each={rows}>{(row) => <li class={selected === row().id ? \"on\" : \"\"} onClick={() => selected = row().id} />}</For>\n    <Show when={open} fallback={<i>closed</i>}><p onClick={() => open = false}>open</p></Show>\n    <button onClick={() => { rows = [...rows, { id: rows.length }]; open = !open; }} />\n  </ul>\n);",
    ),
    (
        "async_component",
        "import { $signal } from \"reze-js\";\nexport async function Card(props) {\n  let n = $signal(1);\n  const user = await fetchUser(props.id);\n  return <p title={n} onClick={() => n += 1}>{user.name}{n}</p>;\n}",
    ),
    (
        "folded_when_never_written",
        "import { $signal } from \"reze-js\";\nlet title = $signal(\"Reze\");\nconst fixed = $signal(1);\nlet count = $signal(0);\nexport const view = <h1 onClick={() => count += 1}>{title}: {fixed} {count}</h1>;",
    ),
    (
        "typed_declaration",
        "import { $signal } from \"reze-js\";\nlet count: number | undefined = $signal();\nlet list: string[] = $signal<string[]>([]);\nexport const view = <p onClick={() => count = 1}>{count}{list}</p>;",
    ),
    (
        "no_jsx",
        "import { $signal } from \"reze-js\";\nlet count = $signal(0);\nexport const inc = () => { count += 1; };\nexport const read = () => count;",
    ),
    (
        "read_once_warning",
        "import { $signal, computed } from \"reze-js\";\nlet count = $signal(0);\nexport function Counter() {\n  const doubled = count * 2;\n  const live = computed(() => count * 2);\n  const view = <b>{count}</b>;\n  const again = $signal(count);\n  return <p onClick={() => count += 1}>{doubled}{live()}</p>;\n}",
    ),
];

fn render(source: &str) -> String {
    let out = match compile(source, "case.tsx", &options()) {
        Ok(Some(out)) => out,
        Ok(None) => return "<no JSX>".to_string(),
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
    let source = "import { $signal, computed } from \"reze-js\";\nlet count = $signal(0);\nexport function Counter() {\n  const copy = count;\n  const live = computed(() => count * 2);\n  const view = <b>{count}</b>;\n  const seeded = $signal(count);\n  const handler = () => { const inner = count; };\n  return view;\n}\nfunction helper() {\n  const plain = count;\n  return plain;\n}";
    let out = compile(source, "test.tsx", &options()).unwrap().unwrap();
    let once: Vec<_> = out.diagnostics.iter().filter(|d| d.code == Code::SignalReadOnce).collect();
    assert_eq!(once.len(), 1, "{once:?}");
    assert_eq!(once[0].start.line, 4);
    assert_eq!(once[0].severity, Severity::Warn);
    assert_eq!(once[0].data["variable"], "copy");
}

#[test]
fn diagnostics_of_the_second_pass_point_into_the_original_source() {
    let source = "import { $signal, For } from \"reze-js\";\n\nlet selected = $signal(0);\nexport const view = (\n  <For each={rows()}>{(row) => <li class={selected === row().id ? \"on\" : \"\"} onClick={() => selected = row().id} />}</For>\n);";
    let out = compile(source, "test.tsx", &options()).unwrap().unwrap();
    let auto = out.diagnostics.iter().find(|d| d.code == Code::AutoSelector).expect("selector");
    let line = source.lines().nth(auto.start.line as usize - 1).unwrap();
    assert_eq!(
        &line[auto.start.column as usize..],
        "selected === row().id ? \"on\" : \"\"} onClick={() => selected = row().id} />}</For>"
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
    assert!(out.code.contains("const [count, setCount] = signal(0);"), "{}", out.code);
    assert!(out.code.contains("() => count()"), "{}", out.code);
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

/// Each pair is a `$computed` module and what a person writes with `computed`; both must compile
/// to the same text.
const COMPUTED_INVARIANT: &[(&str, &str, &str)] = &[
    (
        "reads_in_jsx_handler_and_shorthand",
        "import { $computed, $signal } from \"reze-js\";\nlet count = $signal(0);\nconst doubled = $computed(count * 2);\nexport const view = <button title={doubled} onClick={() => { log({ doubled }); count += doubled; }}>{doubled}</button>;",
        "import { computed, signal } from \"reze-js\";\nconst [count, setCount] = signal(0);\nconst doubled = computed(() => count() * 2);\nexport const view = <button title={doubled()} onClick={() => { log({ doubled: doubled() }); setCount(count() + doubled()); }}>{doubled()}</button>;",
    ),
    (
        "derived_from_derived_object_and_options",
        "import { $computed, $signal } from \"reze-js\";\nlet w = $signal(1);\nconst area = $computed(w * w, { name: \"area\" });\nconst box = $computed({ w, area }, { name: `box${w}` });\nexport const view = <p onClick={() => { w++; }}>{box.area}</p>;",
        "import { computed, signal } from \"reze-js\";\nconst [w, setW] = signal(1);\nconst area = computed(() => w() * w(), { name: \"area\" });\nconst box = computed(() => ({ w: w(), area: area() }), { name: `box${w()}` });\nexport const view = <p onClick={() => { setW(w() + 1); }}>{box().area}</p>;",
    ),
    (
        "let_and_type_annotation",
        "import { $computed, $signal } from \"reze-js\";\nlet n = $signal(1);\nlet half: number = $computed(n / 2);\nexport const view = <p onClick={() => { n++; }}>{half}</p>;",
        "import { computed, signal } from \"reze-js\";\nconst [n, setN] = signal(1);\nconst half = computed<number>(() => n() / 2);\nexport const view = <p onClick={() => { setN(n() + 1); }}>{half()}</p>;",
    ),
    (
        "merged_with_computed_import",
        "import { $computed, computed, signal } from \"reze-js\";\nconst [a, setA] = signal(1);\nconst b = computed(() => a() + 1);\nconst c = $computed(a() + b());\nexport const view = <p onClick={() => setA(2)}>{c}</p>;",
        "import { computed, signal } from \"reze-js\";\nconst [a, setA] = signal(1);\nconst b = computed(() => a() + 1);\nconst c = computed(() => a() + b());\nexport const view = <p onClick={() => setA(2)}>{c()}</p>;",
    ),
    (
        "only_computed_imported",
        "import { $computed } from \"reze-js\";\nimport { signal } from \"@rezejs/signals\";\nconst [a, setA] = signal(1);\nconst c = $computed(a() * 3);\nexport const view = <p onClick={() => setA(2)}>{c}</p>;",
        "import { computed } from \"reze-js\";\nimport { signal } from \"@rezejs/signals\";\nconst [a, setA] = signal(1);\nconst c = computed(() => a() * 3);\nexport const view = <p onClick={() => setA(2)}>{c()}</p>;",
    ),
    (
        "aliased_import",
        "import { $computed as derive, $signal } from \"reze-js\";\nlet a = $signal(1);\nconst c = derive(a + 1);\nexport const view = <p onClick={() => { a++; }}>{c}</p>;",
        "import { computed as derive, signal } from \"reze-js\";\nconst [a, setA] = signal(1);\nconst c = derive(() => a() + 1);\nexport const view = <p onClick={() => { setA(a() + 1); }}>{c()}</p>;",
    ),
    (
        "namespace_import",
        "import * as R from \"reze-js\";\nlet a = R.$signal(1);\nconst c = R.$computed(a + 1);\nexport const view = <p onClick={() => { a++; }}>{c}</p>;",
        "import * as R from \"reze-js\";\nconst [a, setA] = R.signal(1);\nconst c = R.computed(() => a() + 1);\nexport const view = <p onClick={() => { setA(a() + 1); }}>{c()}</p>;",
    ),
    (
        "for_selector_and_show",
        "import { $computed, $signal, For, Show } from \"reze-js\";\nlet picked = $signal(0);\nconst selected = $computed(picked + 1);\nconst big = $computed(picked > 3);\nexport const view = (\n  <ul>\n    <For each={rows()}>{(row) => <li class={selected === row().id ? \"on\" : \"\"} onClick={() => picked = row().id} />}</For>\n    <Show when={big}><p>big</p></Show>\n  </ul>\n);",
        "import { computed, signal, For, Show } from \"reze-js\";\nconst [picked, setPicked] = signal(0);\nconst selected = computed(() => picked() + 1);\nconst big = computed(() => picked() > 3);\nexport const view = (\n  <ul>\n    <For each={rows()}>{(row) => <li class={selected() === row().id ? \"on\" : \"\"} onClick={() => setPicked(() => row().id)} />}</For>\n    <Show when={big()}><p>big</p></Show>\n  </ul>\n);",
    ),
    (
        "async_component_read_after_await",
        "import { $computed, $signal } from \"reze-js\";\nexport async function Card(props) {\n  let n = $signal(1);\n  const twice = $computed(n * 2);\n  const user = await fetchUser(props.id);\n  return <p onClick={() => n += 1}>{user.name}{twice}</p>;\n}",
        "import { computed, signal } from \"reze-js\";\nexport async function Card(props) {\n  const [n, setN] = signal(1);\n  const twice = computed(() => n() * 2);\n  const user = await fetchUser(props.id);\n  return <p onClick={() => setN(n() + 1)}>{user.name}{twice()}</p>;\n}",
    ),
];

#[test]
fn computed_compiles_to_what_a_person_writes_with_computed() {
    for (name, dsl, manual) in COMPUTED_INVARIANT {
        assert_eq!(compiled(dsl, &options()), compiled(manual, &options()), "{name}");
    }
}

#[test]
fn computed_debug_names_name_the_declared_variable() {
    let options = Options { debug_names: true, ..options() };
    let dsl = "import { $computed, $signal } from \"reze-js\";\nlet count = $signal(0);\nconst doubled = $computed(count * 2);\nexport const view = <button onClick={() => count += 1}>{doubled}</button>;";
    let manual = "import { computed, signal } from \"reze-js\";\nconst [count, setCount] = signal(0);\nconst doubled = computed(() => count() * 2);\nexport const view = <button onClick={() => setCount(count() + 1)}>{doubled()}</button>;";
    let code = compiled(dsl, &options);
    assert!(code.contains("{ name: \"doubled\" }"), "{code}");
    assert_eq!(code, compiled(manual, &options));
}

#[test]
fn computed_output_snapshots() {
    let cases = [
        (
            "computed_counter",
            "import { $computed, $signal } from \"reze-js\";\nlet count = $signal(0);\nconst doubled = $computed(count * 2);\nconst label = $computed({ text: `x${doubled}` }, { name: \"label\" });\nexport const view = <button onClick={() => count += 1}>{label.text}: {doubled}</button>;",
        ),
        (
            "computed_async_component",
            "import { $computed, $signal } from \"reze-js\";\nexport async function Card(props) {\n  let n = $signal(1);\n  const user = await fetchUser(props.id);\n  const title = $computed(user.name + n);\n  return <p title={title} onClick={() => n += 1}>{title}</p>;\n}",
        ),
    ];
    for (name, source) in cases {
        insta::assert_snapshot!(name, render(source), source);
    }
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
    assert!(compiled(&fixed, &options()).contains("computed(() => ({ n: count() }))"));

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
    assert!(
        compiled(nested, &options())
            .contains("computed(() => pick(async () => await fetchUser(id)))")
    );
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
