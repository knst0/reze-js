use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use reze_compiler::{Code, Diagnostic, Options, Output, Severity, Target, compile};

fn output(source: &str) -> Output {
    compile(source, "test.tsx", &Options::default()).expect("compiles").expect("has JSX")
}

fn run(source: &str) -> String {
    let code = output(source).code;
    assert_valid(&code, SourceType::tsx());
    code
}

fn run_for(target: Target, source: &str) -> String {
    let options = Options { target, ..Options::default() };
    let code = compile(source, "test.tsx", &options).expect("compiles").expect("has JSX").code;
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
    assert!(
        code.contains("_$style") && code.contains(r#"_$toggleClass(_el$, "on", !!(on()), _p$)"#),
        "{code}"
    );
    let code = run("const a = <div class={cls()} />;");
    assert!(code.contains("_$className"), "{code}");
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
fn duplicate_attributes_keep_the_last() {
    let out = output("const a = <i class=\"a\" class={cls()} title=\"x\" title={t()} />;");
    assert!(out.code.contains("var _v$ = cls(),"), "{}", out.code);
    assert!(!out.code.contains("title=\\\"x"), "{}", out.code);
    assert_eq!(codes(&out.diagnostics), [Code::DuplicateAttribute, Code::DuplicateAttribute]);
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
fn a_computed_read_once_in_reactive_jsx_is_inlined() {
    let source = "import { computed, signal } from \"reze-js\";\n\
                  function Counter() {\n\
                    const [count, setCount] = signal(0);\n\
                    const doubled = computed(() => count() * 2);\n\
                    const label = computed(() => `n${count()}`);\n\
                    const size = computed(() => count() > 9);\n\
                    return <p title={label()} onClick={() => setCount(1)}><Badge big={size()} />{doubled()}</p>;\n\
                  }";
    let out = output(source);
    assert_valid(&out.code, SourceType::tsx());
    assert!(!out.code.contains("computed("), "{}", out.code);
    for inlined in ["(count() * 2)", "(`n${count()}`)", "(count() > 9)"] {
        assert_eq!(out.code.matches(inlined).count(), 1, "{inlined}\n{}", out.code);
    }
    assert!(out.code.contains(r#""" + ((count() * 2))"#), "{}", out.code);
    let inlined: Vec<_> =
        out.diagnostics.iter().filter(|d| d.code == Code::ComputedInlined).collect();
    assert_eq!(inlined.len(), 3);
    assert!(inlined[0].data.contains(&("computed".into(), "doubled".into())), "{:?}", inlined[0]);
    assert!(inlined[0].data.contains(&("scope".into(), "module".into())), "{:?}", inlined[0]);

    let plain = compile(source, "a.tsx", &Options { optimize: false, ..Options::default() })
        .unwrap()
        .unwrap();
    assert!(plain.code.contains("const doubled = computed(() => count() * 2);"), "{}", plain.code);
    assert!(!plain.diagnostics.iter().any(|d| d.code == Code::ComputedInlined));
    assert_valid(&plain.code, SourceType::tsx());
}

#[test]
fn computeds_whose_inlining_could_change_meaning_are_kept() {
    for source in [
        "function A() { const d = computed(() => x()); return <p title={d()}>{d()}</p>; }",
        "function A() { const d = computed(() => x()); log(d()); return <p>{x()}</p>; }",
        "function A() { const d = computed(() => x()); return <For each={xs()}>{() => d()}</For>; }",
        "function A() { const d = computed(() => x()); { const x = y; return <p>{d()}</p>; } }",
        "export const d = computed(() => x()); export const a = <p>{d()}</p>;",
        "const d = computed(() => x()); export { d }; export const a = <p>{d()}</p>;",
        "function A() { const d: () => number = computed(() => x()); return <p>{d()}</p>; }",
        "function A() { const d = computed<number>(() => x()); return <p>{d()}</p>; }",
        "function A() { const d = computed(() => { return x(); }); return <p>{d()}</p>; }",
        "function A() { const d = computed(() => x(), { equals: eq }); return <p>{d()}</p>; }",
        "function A() { let d = computed(() => x()); return <p>{d()}</p>; }",
        "function A() { const d = computed(() => x()), e = 1; return <p>{d()}{e}</p>; }",
        "function A() { const d = computed(() => x()); return <p onClick={d()} />; }",
        "function A() { const d = computed(() => x()); return <p ref={d()} />; }",
        "function A() { const d = computed(() => x()); return <p {...d()} />; }",
        "function A() { const d = computed(() => x()); return <p>{false && d()}</p>; }",
        "function A() { const d = computed(() => x()); return <p title={d()} title=\"y\" />; }",
        "function A() { const d = computed(() => x()); return <p>{d?.()}</p>; }",
        "const d = computed(() => x()); function A() { return <p>{d()}</p>; }",
        "async function A() { const d = computed(() => x()); await f(); return <p>{d()}</p>; }",
        "function A() { const a = <p>{d()}</p>; const d = computed(() => x()); return a; }",
    ] {
        let out = output(&format!("import {{ computed }} from \"reze-js\";\n{source}"));
        assert_valid(&out.code, SourceType::tsx());
        assert!(
            !out.diagnostics.iter().any(|d| d.code == Code::ComputedInlined),
            "{source}\n{}",
            out.code
        );
        assert!(out.code.contains("computed"), "{source}\n{}", out.code);
    }
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
        (Code::UnknownAttribute, "const a = <div className=\"x\" />;"),
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
        (Code::PropsDestructured, "function Greeting({ name = f() }) { return <p>{name}</p>; }"),
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
fn destructured_props_become_reactive_reads() {
    let out = output(
        "function Greeting({ name, count = 1 }) {\n  return <p title={name}>{name}{count}</p>;\n}",
    );
    let code = &out.code;
    assert!(code.contains("function Greeting(_props$)"), "{code}");
    assert_eq!(templates(code), ["<p></p>"], "{code}");
    assert!(code.contains("() => _props$.name"), "{code}");
    assert!(code.contains("(_props$.count === undefined ? 1 : _props$.count)"), "{code}");
    assert!(code.contains("_$bind("), "{code}");
    let rewritten: Vec<_> =
        out.diagnostics.iter().filter(|d| d.code == Code::PropsRewritten).collect();
    assert_eq!(rewritten.len(), 1, "{:?}", out.diagnostics);
    assert!(rewritten[0].data.contains(&("component".into(), "Greeting".into())));
    assert!(codes(&out.diagnostics).is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn props_destructuring_that_cannot_be_rewritten_warns_with_its_reason() {
    let cases = [
        ("computed-key", "function C({ [k]: a }) { return <p>{a}</p>; }"),
        ("default", "function C({ a = f() }) { return <p>{a}</p>; }"),
        ("default", "function C({ a = new Date() }) { return <p>{a}</p>; }"),
        ("default", "function C({ a = tag`x` }) { return <p>{a}</p>; }"),
        ("default", "function C({ a = <b /> }) { return <p>{a}</p>; }"),
        ("default", "function C({ a = b, b }) { return <p>{a}{b}</p>; }"),
        ("default", "function C({ a = x }) { const x = 1; return <p>{a}{x}</p>; }"),
        ("default", "const C = ({ a = x }) => { if (a) { var x; } return <p>{a}</p>; };"),
        ("arguments", "function C({ a = arguments[0] }) { return <p>{a}</p>; }"),
        ("nested-default", "function C({ a: { b } = {} }) { return <p>{b}</p>; }"),
        ("nested-rest", "function C({ a: { ...b } }) { return <p>{b}</p>; }"),
        ("written", "function C({ a }) { a = 1; return <p>{a}</p>; }"),
        ("written", "function C({ a }) { var a = 2; return <p>{a}</p>; }"),
        ("arguments", "function C({ a }) { return <p>{(() => arguments[0])()}{a}</p>; }"),
        ("generator", "function* C({ a }) { yield <p>{a}</p>; }"),
    ];
    for (reason, source) in cases {
        let out = output(source);
        assert_eq!(codes(&out.diagnostics), [Code::PropsDestructured], "{source}");
        let warning = &out.diagnostics[0];
        assert!(warning.data.contains(&("reason".into(), reason.into())), "{source}: {warning:?}");
        assert!(!out.code.contains("_props$"), "{}", out.code);
    }
    let own_arguments = output(
        "function C({ a }) { const f = function () { return arguments[0]; }; return <p>{f()}{a}</p>; }",
    );
    assert!(own_arguments.code.contains("function C(_props$)"), "{}", own_arguments.code);
}

#[test]
fn pure_props_defaults_run_once_at_the_start_and_reads_stay_lazy() {
    let out = output(
        "function Badge({ label, color = theme.color, text = `${label}!`, onPick = () => pick(later), tone = color, n = 1, later }) {\n  \"use client\";\n  return <b title={text} onClick={onPick}>{tone}{n}{later}</b>;\n}",
    );
    let code = &out.code;
    assert_valid(code, SourceType::tsx());
    assert!(codes(&out.diagnostics).is_empty(), "{:?}", out.diagnostics);
    assert!(
        code.contains(
            "\"use client\"; const _color$default = theme.color; const _text$default = `${_props$.label}!`; const _onPick$default = () => pick(_props$.later); const _tone$default = (_props$.color === undefined ? _color$default : _props$.color);"
        ),
        "{code}"
    );
    assert_eq!(code.matches("theme.color").count(), 1, "{code}");
    assert!(code.contains("(_props$.tone === undefined ? _tone$default : _props$.tone)"), "{code}");
    assert!(code.contains("(_props$.n === undefined ? 1 : _props$.n)"), "{code}");
    let arrow = run("const C = ({ a = x }) => <p>{a}</p>;");
    assert!(arrow.contains("const C = (_props$) => { const _a$default = x; return ("), "{arrow}");
}

#[test]
fn class_aliases_are_unknown_attributes() {
    for alias in ["className", "classList"] {
        let out = output(&format!("const a = <i {alias}=\"x\" />;"));
        assert_eq!(codes(&out.diagnostics), [Code::UnknownAttribute], "{alias}");
        assert_eq!(templates(&out.code), [format!("<i {alias}=\"x\"></i>")], "{alias}");
    }
}

#[test]
fn rendered_diagnostics_carry_code_path_frame_and_fix() {
    let out = output("function App() {\n  return <ul><li classList={{ on: true }} /></ul>;\n}");
    let rendered = &out.diagnostics[0].rendered;
    assert!(rendered.starts_with("[UNKNOWN_ATTRIBUTE] "), "{rendered}");
    assert!(rendered.contains("\n  in <App> › ul › li\n"), "{rendered}");
    assert!(rendered.contains("\n  at test.tsx:2:18\n"), "{rendered}");
    assert!(rendered.contains("> 2 |"), "{rendered}");
    assert!(rendered.contains("\n  fix: rename to `class`"), "{rendered}");
    assert!(out.diagnostics[0].docs().ends_with("SKILL.md#unknown_attribute"));
}

#[test]
fn server_output_never_evaluates_client_only_attributes() {
    let code = run_for(
        Target::Server,
        "const a = <div ref={refs[i++]} onClick={go} on:scroll={scroll} prop:x={x()}>{a()}</div>;",
    );
    for client_only in ["refs", "go", "scroll", "x()"] {
        assert!(!code.contains(client_only), "{client_only} in\n{code}");
    }
    assert!(code.contains("_$ssrChild(a())"), "{code}");
}

#[test]
fn server_brackets_every_insert_but_a_sole_child() {
    let sole = run_for(Target::Server, "const a = <p>{a()}</p>;");
    assert!(!sole.contains("<!--["), "{sole}");
    let shared = run_for(Target::Server, "const a = <p>{a()}{b()}<i /></p>;");
    assert_eq!(shared.matches("<!--[-->").count(), 2, "{shared}");
    assert_eq!(shared.matches("<!--]-->").count(), 2, "{shared}");
}

#[test]
fn hydrate_skips_the_ranges_of_later_inserts_sharing_an_anchor() {
    let code = run_for(Target::Hydrate, "const a = <p>{a()}{b()}{c()}<i /></p>;");
    assert!(code.contains("_$claimInsert(_el$, a, _el$2, 2)"), "{code}");
    assert!(code.contains("_$claimInsert(_el$, b, _el$2, 1)"), "{code}");
    assert!(code.contains("_$claimInsert(_el$, c, _el$2)"), "{code}");
}

#[test]
fn inserts_run_in_document_order() {
    let code = run("const a = <div>{a()}<p>{b()}</p>{c()}</div>;");
    let position = |call: &str| code.find(call).unwrap_or_else(|| panic!("{call} in\n{code}"));
    assert!(position("_$insert(_el$, a") < position("_$insert(_el$2, b"), "{code}");
    assert!(position("_$insert(_el$2, b") < position("_$insert(_el$, c"), "{code}");
}

fn store_diagnostic(diagnostics: &[Diagnostic]) -> Option<&Diagnostic> {
    diagnostics.iter().find(|d| d.code == Code::StoreUnproxied)
}

fn data<'d>(diagnostic: &'d Diagnostic, key: &str) -> Option<&'d str> {
    diagnostic.data.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

#[test]
fn a_store_used_only_at_its_leaves_becomes_one_signal_per_leaf() {
    let source = "import { store } from \"reze-js\";\n\
                  const [todo, setTodo] = store({ title: \"\", meta: { count: 0, \"last-seen\": null } });\n\
                  const a = <p onClick={() => setTodo((d) => { d.meta.count += d.meta.count; d.meta[\"last-seen\"] ??= now(); d.meta.count++; })}>{todo.title}</p>;\n\
                  const rename = (t) => setTodo((d) => d.title = t);";
    let out = output(source);
    assert_valid(&out.code, SourceType::tsx());
    let code = &out.code;
    assert!(
        code.contains(
            "const [todo$title, setTodo$title] = _$signal(\"\"), [todo$meta$count, setTodo$meta$count] = _$signal(0), [todo$meta$last_seen, setTodo$meta$last_seen] = _$signal(null);"
        ),
        "{code}"
    );
    assert!(code.contains("void _$untrack(() => { setTodo$meta$count((v) => v + todo$meta$count()); setTodo$meta$last_seen((v) => v ?? now()); setTodo$meta$count((v) => ++v); })"), "{code}");
    assert!(!code.contains("setTodo((d) =>"), "{code}");
    assert!(code.contains("void _$untrack(() => setTodo$title(() => t))"), "{code}");
    assert!(code.contains("todo$title()"), "{code}");
    assert!(!code.contains("store("), "{code}");
    let diagnostic = store_diagnostic(&out.diagnostics).expect("STORE_UNPROXIED");
    assert_eq!(diagnostic.severity, Severity::Info);
    assert_eq!(data(diagnostic, "store"), Some("todo"));
    assert_eq!(data(diagnostic, "scope"), Some("module"));
}

#[test]
fn a_read_only_store_declares_no_setters_and_namespace_store_is_recognized() {
    let code = run("import * as R from \"reze-js\";\n\
                    function Badge() { const [s] = R.store({ label: \"x\", size: { w: 1 } }); return <b title={(s.size).w}>{(s.label)}</b>; }");
    assert!(
        code.contains("const [s$label] = _$signal(\"x\"), [s$size$w] = _$signal(1);"),
        "{code}"
    );
    assert!(
        code.contains("s$label()") && code.contains("s$size$w()") && !code.contains("s.label"),
        "{code}"
    );
}

#[test]
fn stores_with_any_other_use_keep_the_proxy() {
    let prelude = "import { store } from \"reze-js\";\n";
    for source in [
        "const [s] = store({ a: 1 }); use(s); const x = <p>{s.a}</p>;",
        "const [s] = store({ a: { b: 1 } }); const x = <p>{s.a}</p>;",
        "const [s] = store({ a: 1 }); const x = <p>{s[key]}</p>;",
        "const [s] = store({ items: [] }); const x = <ul>{s.items.map((i) => <li>{i}</li>)}</ul>;",
        "const [s, set] = store({ a: 1 }); set((d) => { later(() => { d.a = 2; }); }); const x = <p>{s.a}</p>;",
        "const [s, set] = store({ a: 1 }); set((d) => { use(d.a = 2); }); const x = <p>{s.a}</p>;",
        "const [s, set] = store({ a: 1 }); set((d) => { d.b = 2; }); const x = <p>{s.a}</p>;",
        "const [s, set] = store({ a: 1 }); set((d) => { this.x = d.a; }); const x = <p>{s.a}</p>;",
        "const [s, set] = store({ a: 1 }); set(async (d) => { d.a = 2; }); const x = <p>{s.a}</p>;",
        "const [s, set] = store({ a: 1 }); set(update); const x = <p>{s.a}</p>;",
        "const [s] = store({ a: 1 }); const x = <p ref={s.a} />;",
        "const [s] = store({ ...base }); const x = <p>{s.a}</p>;",
        "const [s] = store({ [k]: 1 }); const x = <p>{s.a}</p>;",
        "let [s] = store({ a: 1 }); const x = <p>{s.a}</p>;",
        "export const [s] = store({ a: 1 }); const x = <p>{s.a}</p>;",
        "const [s] = store({ a: 1 }); export { s }; const x = <p>{s.a}</p>;",
    ] {
        let out = output(&format!("{prelude}{source}"));
        assert_valid(&out.code, SourceType::tsx());
        assert!(store_diagnostic(&out.diagnostics).is_none(), "{source}\n{}", out.code);
        assert!(out.code.contains("store("), "{}", out.code);
    }
}

#[test]
fn without_optimize_a_store_keeps_its_proxy() {
    let source = "import { store } from \"reze-js\";\nconst [s, set] = store({ a: 1 });\n\
                  const x = <p onClick={() => set((d) => { d.a++; })}>{s.a}</p>;";
    let options = Options { optimize: false, ..Options::default() };
    let out = compile(source, "test.tsx", &options).unwrap().unwrap();
    assert_valid(&out.code, SourceType::tsx());
    assert!(out.code.contains("store({ a: 1 })"), "{}", out.code);
    assert!(out.code.contains("set((d) => { d.a++; })"), "{}", out.code);
    assert!(store_diagnostic(&out.diagnostics).is_none());
}

fn program_options(source: &str, facts: reze_compiler::facts::ModuleFacts) -> Options {
    use reze_compiler::facts::{VERSION, source_hash};
    let facts = reze_compiler::facts::ModuleFacts {
        version: VERSION.to_string(),
        source_hash: source_hash(source),
        ..facts
    };
    Options { source_map: false, facts: Some(facts), ..Options::default() }
}

fn leaf(
    path: &[&str],
    getter: Option<&str>,
    setter: Option<&str>,
) -> reze_compiler::facts::LeafNames {
    reze_compiler::facts::LeafNames {
        path: path.iter().map(|k| k.to_string()).collect(),
        getter: getter.map(str::to_string),
        setter: setter.map(str::to_string),
        is_array: false,
    }
}

#[test]
fn an_exported_store_the_program_unproxies_exports_its_leaves() {
    use reze_compiler::facts::{ModuleFacts, StoreExport};
    let source = "import { store } from \"reze-js\";\n\
                  export const [state, setState] = store({ count: 0, user: { name: \"a\" } });\n\
                  export function bump() { setState((d) => { d.count++; }); }\n";
    let related = reze_compiler::Related {
        file: "app.tsx".to_string(),
        start: 1,
        end: 5,
        message: "read here".to_string(),
    };
    let facts = ModuleFacts {
        stores: vec![StoreExport {
            state: source.find("state").unwrap() as u32,
            leaves: vec![
                leaf(&["count"], Some("state$count"), Some("setState$count")),
                leaf(&["user", "name"], Some("state$user$name"), None),
            ],
            related: vec![related.clone()],
        }],
        ..ModuleFacts::default()
    };
    let out = compile(source, "store.ts", &program_options(source, facts)).unwrap().unwrap();
    assert_valid(&out.code, SourceType::ts());
    assert!(out.code.contains(
        "const [state$count, setState$count] = _$signal(0), [state$user$name] = _$signal(\"a\");\nexport { state$count, setState$count, state$user$name };"
    ), "{}", out.code);
    assert!(
        out.code.contains("void _$untrack(() => { setState$count((v) => ++v); })"),
        "{}",
        out.code
    );
    let diagnostic = store_diagnostic(&out.diagnostics).expect("STORE_UNPROXIED");
    assert_eq!(data(diagnostic, "scope"), Some("program"));
    assert_eq!(diagnostic.related, [related]);

    let source = "import { store } from \"reze-js\";\n\
                  const [state, setState] = store({ count: 0 });\n\
                  export { state, setState as set, other };\nconst other = 1;\n";
    let facts = ModuleFacts {
        stores: vec![StoreExport {
            state: source.find("state").unwrap() as u32,
            leaves: vec![leaf(&["count"], Some("state$count"), None)],
            related: Vec::new(),
        }],
        ..ModuleFacts::default()
    };
    let out = compile(source, "store.ts", &program_options(source, facts)).unwrap().unwrap();
    assert_valid(&out.code, SourceType::ts());
    assert!(out.code.contains("const [state$count] = _$signal(0);"), "{}", out.code);
    assert!(out.code.contains("export { state$count, other };"), "{}", out.code);
}

#[test]
fn an_importer_of_an_unproxied_store_imports_its_leaves() {
    use reze_compiler::facts::{ModuleFacts, StoreImport, StoreRole};
    let source = "import { setState as set, state } from \"./store\";\n\
                  export const App = () => <button onClick={() => set((d) => { d.count += 1; })}>{state.count}</button>;\n";
    let facts = ModuleFacts {
        store_imports: vec![
            StoreImport {
                binding: (source.find("as set").unwrap() + 3) as u32,
                role: StoreRole::Setter,
                leaves: vec![leaf(&["count"], None, Some("setState$count"))],
            },
            StoreImport {
                binding: source.find("state }").unwrap() as u32,
                role: StoreRole::State,
                leaves: vec![leaf(&["count"], Some("state$count"), None)],
            },
        ],
        ..ModuleFacts::default()
    };
    let out = compile(source, "app.tsx", &program_options(source, facts)).unwrap().unwrap();
    assert_valid(&out.code, SourceType::tsx());
    assert!(
        out.code.starts_with(
            "import { setState$count as _setState$count, state$count as _state$count } from \"./store\";"
        ),
        "{}",
        out.code
    );
    assert!(
        out.code.contains("void _$untrack(() => { _setState$count((v) => v + 1); })"),
        "{}",
        out.code
    );
    assert!(out.code.contains("_state$count()"), "{}", out.code);
    assert!(store_diagnostic(&out.diagnostics).is_none());
}

#[test]
fn facts_built_for_another_source_are_stale() {
    let summary =
        reze_compiler::summarize("const a = <p />;", "a.tsx", &Default::default()).unwrap();
    let module = reze_compiler::ModuleInput {
        id: "a.tsx".into(),
        summary,
        resolved: Vec::new(),
        is_entry: true,
    };
    let linked = reze_compiler::link(
        &[module],
        &reze_compiler::LinkOptions { optimize: true, islands: false, root: String::new() },
    );
    let options = Options { facts: Some(linked.facts["a.tsx"].clone()), ..Options::default() };
    let errors = compile("const a = <b />;", "a.tsx", &options).err().expect("stale facts");
    assert_eq!(codes(&errors), [Code::FactsStale]);
    assert!(compile("const a = <p />;", "a.tsx", &options).is_ok());
}

#[test]
fn exported_signals_do_not_fold_without_the_program() {
    for source in [
        "import { signal } from \"reze-js\";\nexport const [title] = signal(\"x\");\nconst a = <p>{title()}</p>;",
        "import { signal } from \"reze-js\";\nconst [title] = signal(\"x\");\nexport { title };\nconst a = <p>{title()}</p>;",
        "import { signal } from \"reze-js\";\nconst [title, setTitle] = signal(\"x\");\nexport { setTitle };\nconst a = <p>{title()}</p>;",
    ] {
        let out = output(source);
        assert!(out.code.contains("signal(\"x\")"), "{}", out.code);
        assert!(!out.diagnostics.iter().any(|d| d.code == Code::SignalFolded), "{source}");
    }
}

#[test]
fn verify_reports_only_real_closed_world_and_flag_breaks() {
    let summary =
        |source: &str| reze_compiler::summarize(source, "/o.ts", &Default::default()).unwrap();
    let linked = reze_compiler::Linked {
        facts: Default::default(),
        features: [("loading".to_string(), false)].into_iter().collect(),
        closed: vec!["/state.ts".to_string()],
    };
    let outside = |source: &str, imported: &str| reze_compiler::OutsideModule {
        id: "/o.ts".into(),
        summary: Some(summary(source)),
        imported: vec![imported.to_string()],
    };
    let found =
        |modules: &[reze_compiler::OutsideModule]| codes(&reze_compiler::verify(&linked, modules));
    assert_eq!(found(&[outside("export const a = 1;", "/other.ts")]), []);
    assert_eq!(found(&[outside("export const a = 1;", "/state.ts")]), [Code::ProgramOpenImport]);
    assert_eq!(
        found(&[outside("import * as R from \"reze-js\";\nexport const S = R.Loading;", "/x.ts")]),
        [Code::FeatureFlagMismatch]
    );
    assert_eq!(
        found(&[outside("import { signal } from \"reze-js\";\nexport { signal };", "/x.ts")]),
        []
    );
}

const ROW_SELECTION: &str = "import { For, signal } from \"reze-js\";\nconst [selected, setSelected] = signal(0);\nsetSelected(1);\nexport const list = <For each={rows()}>{(row) => <li class={selected() === row().id ? \"on\" : \"\"} />}</For>;";

#[test]
fn row_comparison_reads_a_selector() {
    let code = run(ROW_SELECTION);
    assert!(code.contains("(_$selector(selected))"), "{code}");
    assert!(code.contains("_sel$(row().id) ? \"on\" : \"\""), "{code}");
}

#[test]
fn row_comparison_stays_plain_without_optimize() {
    let options = Options { optimize: false, ..Options::default() };
    let code = compile(ROW_SELECTION, "test.tsx", &options).unwrap().unwrap().code;
    assert!(!code.contains("selector"), "{code}");
    assert!(code.contains("selected() === row().id"), "{code}");
}

#[test]
fn row_comparison_stays_plain_for_a_for_outside_the_runtime() {
    let source = ROW_SELECTION.replace(
        "import { For, signal } from \"reze-js\";",
        "import { signal } from \"reze-js\";\nimport { For } from \"./list\";",
    );
    let code = run(&source);
    assert!(!code.contains("selector"), "{code}");
}

fn class_setter(expression: &str) -> &'static str {
    let code = run(&format!(
        "import {{ signal }} from \"reze-js\";\nconst [label] = signal(\"x\");\nconst [n] = signal(1);\nconst a = <i class={{{expression}}} />;"
    ));
    if code.contains("_$setAttribute(_el$, \"class\"") {
        "setAttribute"
    } else if code.contains("_$className") {
        "className"
    } else {
        panic!("no class setter in {code}")
    }
}

#[test]
fn provably_string_class_values_use_set_attribute() {
    for expression in [
        "on() ? \"danger\" : \"\"",
        "`row ${kind()}`",
        "\"row-\" + kind()",
        "kind() + \"-row\"",
        "String(kind())",
        "kind().toString()",
        "price().toFixed(2)",
        "parts().join(\" \")",
        "on() ? `a${x()}` : \"b\"",
    ] {
        assert_eq!(class_setter(expression), "setAttribute", "{expression}");
    }
}

#[test]
fn class_values_of_unknown_kind_keep_class_name() {
    for expression in [
        "cls()",
        "a() + b()",
        "on() ? \"a\" : 1",
        "on() && \"a\"",
        "props.cls",
        "on() ? \"a\" : cls()",
        "obj?.toString()",
        "a() === b()",
    ] {
        assert_eq!(class_setter(expression), "className", "{expression}");
    }
}

#[test]
fn a_shadowed_string_function_is_not_the_global() {
    let code = run("function f(String) { return <i class={String(x())} />; }");
    assert!(code.contains("_$className"), "{code}");
}
