use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use reze_compiler::{Code, Diagnostic, Options, Output, Severity, compile};

fn output(source: &str) -> Output {
    compile(source, "test.tsx", &Options::default()).expect("compiles").expect("has JSX")
}

fn run(source: &str) -> String {
    let code = output(source).code;
    assert_valid(&code, SourceType::tsx());
    code
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<Code> {
    diagnostics.iter().filter(|d| d.severity != Severity::Info).map(|d| d.code).collect()
}

/// The HTML of every `template*()` factory call, in declaration order.
fn templates(code: &str) -> Vec<String> {
    code.split("_$template")
        .skip(1)
        .filter_map(|rest| {
            let rest = rest.strip_prefix("SVG").or(rest.strip_prefix("MathML")).unwrap_or(rest);
            let rest = rest.strip_prefix("(\"")?;
            let mut html = String::new();
            let mut chars = rest.chars();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => match chars.next() {
                        Some('n') => html.push('\n'),
                        Some(c) => html.push(c),
                        None => break,
                    },
                    c => html.push(c),
                }
            }
            Some(html)
        })
        .collect()
}

/// The output parses in its dialect and declares nothing twice.
fn assert_valid(code: &str, source_type: SourceType) {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, source_type).parse();
    assert!(parsed.diagnostics.is_empty(), "{:?}\n{code}", parsed.diagnostics);
    let semantic = SemanticBuilder::new().with_check_syntax_error(true).build(&parsed.program);
    assert!(semantic.diagnostics.is_empty(), "{:?}\n{code}", semantic.diagnostics);
}

fn apply_fixes(source: &str, diagnostic: &Diagnostic) -> String {
    let mut edits: Vec<_> = diagnostic.fixes.iter().flat_map(|fix| fix.edits.iter()).collect();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.start));
    let mut fixed = source.to_string();
    for edit in edits {
        fixed.replace_range(edit.start as usize..edit.end as usize, &edit.text);
    }
    fixed
}

#[test]
fn file_without_jsx_is_left_alone() {
    assert!(compile("const a = 1 < 2;", "a.ts", &Options::default()).unwrap().is_none());
}

#[test]
fn syntax_errors_are_parse_error_diagnostics_at_their_position() {
    let errors = compile("const a = 1;\nconst b = <div>;", "a.tsx", &Options::default())
        .err()
        .expect("invalid JSX");
    assert_eq!(errors[0].code, Code::ParseError);
    assert_eq!(errors[0].severity, Severity::Error);
    assert_eq!(errors[0].start.line, 2);
    assert!(errors[0].message.starts_with("[PARSE_ERROR] "), "{}", errors[0].message);
}

#[test]
fn a_marker_separates_an_insertion_from_texts_the_parser_would_merge() {
    let code = run("const a = <p>hi {name()}!</p>;\nconst b = <p>{a()}:{b()}<i/>{c()}</p>;");
    assert_eq!(templates(&code), ["<p>hi <!>!</p>", "<p>:<i></i></p>"]);
}

#[test]
fn text_follows_jsx_whitespace_and_entity_rules() {
    let code = run(
        "const a = (\n  <p title=\"x &amp; &quot;y&quot;\">\n    a &amp; b\n    c&nbsp;&#x41;&#66; {\"<&>\"}\n  </p>\n);",
    );
    assert_eq!(
        templates(&code),
        ["<p title=\"x &amp; &quot;y&quot;\">a &amp; b c\u{a0}AB &lt;&amp;></p>"]
    );
}

#[test]
fn component_text_children_are_decoded_strings() {
    let code = run("const a = <Button label=\"a &lt; b\">Save &amp; exit</Button>;");
    assert!(code.contains(r#"label: "a < b""#), "{code}");
    assert!(code.contains(r#"children: "Save & exit""#), "{code}");
}

#[test]
fn an_svg_child_template_is_parsed_inside_svg_and_an_svg_root_natively() {
    let code = run("const a = <path d=\"M0\" />;\nconst b = <svg><path /></svg>;");
    assert!(code.contains(r#"_$templateSVG("<svg><path d=\"M0\"></path></svg>")"#), "{code}");
    assert!(code.contains(r#"_$template("<svg><path></path></svg>")"#), "{code}");
}

#[test]
fn a_math_root_uses_the_mathml_factory_but_nested_math_does_not() {
    let code = run("const a = <math><mi>{x}</mi></math>;");
    assert!(code.contains(r#"_$templateMathML("<math><mi></mi></math>")"#), "{code}");
    let code = run("const a = <div><math><mi>x</mi></math></div>;");
    assert!(!code.contains("templateMathML"), "{code}");
}

#[test]
fn generated_names_avoid_names_in_the_source() {
    run("const _tmpl$ = 1, _el$ = 2, _$insert = 3, _p$ = 4, _c$ = 5;\n\
         export const a = <p class={c()} title={t()}>{_tmpl$}{_el$}{_$insert}{_p$}{x() ? <b/> : _c$}<b/></p>;");
}

#[test]
fn string_concatenation_folds_but_number_addition_does_not() {
    let code = run(r#"const a = <div title={"a" + "b" + `c`}>{"x" + 1}</div>;"#);
    assert_eq!(templates(&code), [r#"<div title="abc">x1</div>"#]);
    let code = run("const a = <div title={1 + 2}>{\"x\" + y}</div>;");
    assert!(!code.contains("title=\""), "{code}");
    assert!(code.contains("1 + 2"), "{code}");
}

#[test]
fn runtime_imports_and_templates_follow_the_leading_imports() {
    let code = run("import { a } from \"a\";\nimport b from \"b\";\nconst x = <i/>;");
    let runtime = code.find("from \"reze-js\"").unwrap();
    assert!(code.find("from \"b\"").unwrap() < runtime);
    assert!(runtime < code.find("const x").unwrap());
}

#[test]
fn delegated_events_are_registered_once_per_module_in_sorted_order() {
    let code = run("const a = <><b onInput={f} /><i onClick={() => g()} onClick2={h} /></>;");
    assert!(code.trim_end().ends_with(r#"_$delegateEvents(["click", "input"]);"#), "{code}");
}

#[test]
fn literal_style_and_class_values_fold_into_the_template() {
    let code = run(
        r#"const a = <div style={{color: "red", "font-size": "12px"}} class={["a", {b: true, c: false}, [0, "d e"]]} />;"#,
    );
    assert_eq!(
        templates(&code),
        [r#"<div style="color:red;font-size:12px" class="a b 0 d e"></div>"#]
    );
    assert!(!code.contains("_$style") && !code.contains("_$className"), "{code}");
    let code = run(r#"const a = <i class={["a", { a: false }, "b"]} />;"#);
    assert_eq!(templates(&code), [r#"<i class="b"></i>"#]);
    let code = run("const a = <div style={{color: c}} class={{on: on()}} />;");
    assert!(code.contains("_$style") && code.contains("_$className"), "{code}");
}

#[test]
fn source_map_points_generated_code_back_at_its_jsx() {
    let source = "const n = 1;\nconst a = <p>{n}</p>;\n";
    let out = output(source);
    let map = oxc_sourcemap::SourceMap::from_json_string(out.map.as_deref().unwrap()).unwrap();
    let line = out.code.lines().position(|l| l.starts_with("const a")).unwrap() as u32;
    let token = map
        .get_tokens()
        .find(|t| t.get_dst_line() == line && t.get_dst_col() == 10)
        .expect("token at the compiled JSX");
    assert_eq!((token.get_src_line(), token.get_src_col()), (1, 10));
}

#[test]
fn bare_call_children_skip_the_getter_closure_but_methods_keep_it() {
    let code = run("const a = <div>{f()}</div>;");
    assert!(code.contains("_$insert(_el$, f)"), "{code}");
    let code = run("const a = <div>{obj.m()}</div>;");
    assert!(code.contains("() => obj.m()"), "{code}");
}

#[test]
fn void_and_svg2_elements() {
    assert_eq!(templates(&run("const a = <search />;")), ["<search>"]);
    assert_eq!(
        templates(&run("const a = <hatch><hatchpath /></hatch>;")),
        ["<svg><hatch><hatchpath></hatchpath></hatch></svg>"]
    );
}

#[test]
fn a_ref_expression_is_evaluated_once() {
    let code = run("const a = <div ref={refs[i++]} />;");
    assert_eq!(code.matches("i++").count(), 1, "{code}");
    let code = run("const a = <Comp ref={box.el} />;");
    assert_eq!(code.matches("box").count(), 1, "{code}");
}

#[test]
fn textarea_and_select_values_are_properties_set_after_their_children() {
    let code = run("const a = <textarea value=\"hi\" />;");
    assert_eq!(templates(&code), ["<textarea></textarea>"]);
    assert!(code.contains(r#".value = "hi""#), "{code}");
    let code = run("const a = <select value=\"b\">{options()}</select>;");
    let insert = code.find("_$insert").unwrap();
    assert!(insert < code.find(".value = \"b\"").unwrap(), "{code}");
}

#[test]
fn class_sources_merge_and_duplicates_keep_the_last() {
    let out =
        output("const a = <i class=\"a\" classList={{ on: on() }} title=\"x\" title={t()} />;");
    assert!(out.code.contains(r#"["a", { on: on() }]"#), "{}", out.code);
    assert!(!out.code.contains("title=\\\"x"), "{}", out.code);
    assert_eq!(codes(&out.diagnostics), [Code::ClassAlias, Code::DuplicateAttribute]);
}

#[test]
fn double_click_listens_for_dblclick() {
    let code = run("const a = <div onDoubleClick={f} />;");
    assert!(code.contains(r#"_$addEventListener(_el$, "dblclick", f)"#), "{code}");
}

#[test]
fn async_helpers_without_jsx_keep_their_promise_semantics() {
    let out = output(
        "async function load(u) { const r = await fetch(u); return r.json(); }\nconst el = <div />;",
    );
    assert!(
        out.code.contains("async function load(u) { const r = await fetch(u); return r.json(); }")
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn javascript_output_has_no_type_assertions() {
    let source =
        "async function User(props) { const u = await f(props.id); return <p>{u.name}</p>; }";
    let out = compile(source, "a.jsx", &Options::default()).unwrap().unwrap();
    assert!(!out.code.contains(" as any"), "{}", out.code);
    assert_valid(&out.code, SourceType::jsx());
}

#[test]
fn chained_awaits_become_linked_fetch_steps() {
    let out = output(
        "async function User(props): Promise<string> { \
           const a = await f(props.id); \
           const b = await g(a); \
           return <div>{b}</div>; \
         }",
    );
    assert!(out.code.contains("function User(props): string {"), "{}", out.code);
    assert!(out.code.contains("Promise.resolve(f(props.id))"), "{}", out.code);
    assert!(out.code.contains("Promise.resolve(g(a))"), "{}", out.code);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_valid(&out.code, SourceType::tsx());
}

#[test]
fn async_component_locals_crossing_an_await_keep_async_and_warn() {
    let out = output(
        "const User = async (props) => { \
           const label = props.label; \
           const user = await fetchUser(props.id); \
           return <div>{label}{user.name}</div>; \
         };",
    );
    assert!(out.code.contains("async (props) =>"), "{}", out.code);
    assert_eq!(codes(&out.diagnostics), [Code::AsyncComponentShape]);
}

#[test]
fn a_shadowing_name_after_await_is_not_a_crossing_local() {
    let out = output(
        "async function User(props) { \
           const label = 1; \
           const user = await fetchUser(label); \
           return <div>{[1].map((label) => label)}{user.name}</div>; \
         }",
    );
    assert!(!out.code.contains("async function User"), "{}", out.code);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn constant_signals_fold_into_templates() {
    let source = "import { signal } from \"reze-js\";\n\
                  const [title] = signal(\"Reze\");\n\
                  const [count, setCount] = signal(0); setCount(1);\n\
                  const a = <h1 title={title()}>{title()} {count()}</h1>;";
    let out = output(source);
    assert!(out.code.contains("const title = \"Reze\";"), "{}", out.code);
    assert_eq!(templates(&out.code), ["<h1 title=\"Reze\">Reze </h1>"]);
    assert_eq!(out.diagnostics.iter().filter(|d| d.code == Code::SignalFolded).count(), 1);

    let plain = compile(source, "a.tsx", &Options { optimize: false, ..Options::default() })
        .unwrap()
        .unwrap();
    assert!(plain.code.contains("const [title] = signal(\"Reze\");"), "{}", plain.code);
    assert_valid(&plain.code, SourceType::tsx());
}

#[test]
fn signals_that_are_written_or_escape_do_not_fold() {
    for source in [
        "const [a, setA] = signal(1); setA(2); const x = <p>{a()}</p>;",
        "const [a] = signal(1); use(a); const x = <p>{a()}</p>;",
        "const [a] = signal(1, { equals: eq }); const x = <p>{a()}</p>;",
        "const [a]: [() => number] = signal(1); const x = <p>{a()}</p>;",
    ] {
        let out = output(&format!("import {{ signal }} from \"@rezejs/signals\";\n{source}"));
        assert!(
            !out.diagnostics.iter().any(|d| d.code == Code::SignalFolded),
            "{source}\n{}",
            out.code
        );
    }
    let local =
        output("const signal = (v) => [() => v]; const [a] = signal(1); const x = <p>{a()}</p>;");
    assert!(!local.code.contains("const a = 1"), "{}", local.code);
}

#[test]
fn literal_conditions_drop_dead_branches() {
    let out = output("const a = <div>{false && <b>x</b>}{true ? <i>y</i> : <u>z</u>}{null}</div>;");
    assert_eq!(templates(&out.code), ["<div><i>y</i></div>"]);
    assert_eq!(out.diagnostics.iter().filter(|d| d.code == Code::DeadBranchRemoved).count(), 2);
}

#[test]
fn each_warning_fix_removes_its_warning() {
    let cases = [
        (Code::ClassAlias, "const a = <div className=\"x\" />;"),
        (Code::ChildrenPropIgnored, "const a = <div children={x()}><b /></div>;"),
        (Code::KeyOnElement, "const a = <li key={id} />;"),
        (Code::DuplicateAttribute, "const a = <a href=\"/a\" href={u()} />;"),
        (Code::UnknownAttribute, "const a = <div clas=\"x\" />;"),
        (Code::EventNameLowercase, "const a = <button onclick={() => save()} />;"),
        (
            Code::SignalNotCalled,
            "import { signal } from \"reze-js\";\nconst [n, setN] = signal(0); setN(1);\nconst a = <input value={n} />;",
        ),
    ];
    for (code, source) in cases {
        let out = output(source);
        let diagnostic = out
            .diagnostics
            .iter()
            .find(|d| d.code == code)
            .unwrap_or_else(|| panic!("{code:?}: {:?}", out.diagnostics));
        assert!(!diagnostic.fixes.is_empty(), "{code:?}");
        let fixed = apply_fixes(source, diagnostic);
        let after = output(&fixed);
        assert!(!after.diagnostics.iter().any(|d| d.code == code), "{code:?}: {fixed}");
    }
}

#[test]
fn warnings_without_fixes_are_reported_where_they_apply() {
    let cases = [
        (Code::PropsDestructured, "function Greeting({ name }) { return <p>{name}</p>; }"),
        (Code::InlineEach, "const a = <For each={[1, 2]}>{(n) => n}</For>;"),
        (Code::AsyncComponentShape, "async function A() { await ready(); return <p />; }"),
        (
            Code::AsyncReturnType,
            "async function A(): Promise { const x = await f(); return <p>{x}</p>; }",
        ),
    ];
    for (code, source) in cases {
        let out = output(source);
        assert_eq!(codes(&out.diagnostics), [code], "{source}");
    }
    let quiet = output(
        "const Row = (props) => <li>{props.name}</li>;\nconst a = <For each={items}>{({ name }) => <li>{name}</li>}</For>;",
    );
    assert!(quiet.diagnostics.is_empty(), "{:?}", quiet.diagnostics);
}

#[test]
fn rendered_diagnostics_carry_code_path_frame_and_fix() {
    let out = output("function App() {\n  return <ul><li classList={{ on: true }} /></ul>;\n}");
    let rendered = &out.diagnostics[0].rendered;
    assert!(rendered.starts_with("[CLASS_ALIAS] "), "{rendered}");
    assert!(rendered.contains("\n  in <App> › ul › li\n"), "{rendered}");
    assert!(rendered.contains("\n  at test.tsx:2:18\n"), "{rendered}");
    assert!(rendered.contains("> 2 |"), "{rendered}");
    assert!(rendered.contains("\n  fix: rename `classList` to `class`"), "{rendered}");
    assert!(out.diagnostics[0].docs().ends_with("SKILL.md#class_alias"));
}
