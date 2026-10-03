use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use reze_compiler::{Code, Diagnostic, Options, Output, Severity, compile};

fn output_for(source: &str, filename: &str, options: &Options) -> Output {
    let out = compile(source, filename, options).expect("compiles").expect("rewrites");
    assert_valid(&out.code, SourceType::from_path(filename).unwrap());
    assert!(!out.code.contains("_p$["), "{}", out.code);
    out
}

fn run(source: &str) -> String {
    output_for(source, "test.tsx", &Options::default()).code
}

fn errors(source: &str) -> Vec<Diagnostic> {
    compile(source, "test.tsx", &Options::default()).err().expect("fails")
}

/// The HTML of every template factory call, in source order.
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
                    '\\' => html.extend(chars.next()),
                    c => html.push(c),
                }
            }
            Some(html)
        })
        .collect()
}

fn assert_valid(code: &str, source_type: SourceType) {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, source_type).parse();
    assert!(parsed.diagnostics.is_empty(), "{:?}\n{code}", parsed.diagnostics);
    let semantic = SemanticBuilder::new().with_check_syntax_error(true).build(&parsed.program);
    assert!(semantic.diagnostics.is_empty(), "{:?}\n{code}", semantic.diagnostics);
}

#[test]
fn a_file_without_jsx_is_left_alone() {
    assert!(compile("const a = 1 < 2;", "a.ts", &Options::default()).unwrap().is_none());
}

#[test]
fn syntax_errors_are_parse_errors_at_their_position() {
    let errors = compile("const a = 1;\nconst b = <div>;", "a.tsx", &Options::default())
        .err()
        .expect("fails");
    assert_eq!(errors[0].code, Code::ParseError);
    assert_eq!(errors[0].severity, Severity::Error);
    assert_eq!(errors[0].start.line, 2);
    assert!(errors[0].message.starts_with("[PARSE_ERROR] "), "{}", errors[0].message);
    assert!(errors[0].data.contains_key("detail"));
}

#[test]
fn a_marker_separates_an_insert_from_texts_the_parser_would_merge() {
    let code = run("const a = <p>hi {name()}!</p>;\nconst b = <p>{a()}:{b()}<i/>{c()}</p>;");
    assert_eq!(templates(&code), ["<p>hi <!>!", "<p>:<i>"]);
}

#[test]
fn templates_drop_trailing_end_tags_and_needless_attribute_quotes() {
    let code = run(
        "const a = <div><p class=\"a b\" id=\"x\" title=\"\" data-q=\"a=b\"></p><i>t</i></div>;",
    );
    assert_eq!(templates(&code), [r#"<div><p class="a b" id=x title data-q="a=b"></p><i>t"#]);
}

#[test]
fn text_follows_jsx_whitespace_and_entity_rules() {
    let code = run(
        "const a = (\n  <p title=\"x &amp; &quot;y&quot;\">\n    a &amp; b\n    c&nbsp;&#x41;&#66; {\"<&>\"}\n  </p>\n);",
    );
    assert_eq!(
        templates(&code),
        ["<p title=\"x &amp; &quot;y&quot;\">a &amp; b c\u{a0}AB &lt;&amp;>"]
    );
}

#[test]
fn svg_and_mathml_roots_pick_their_factories() {
    let code = run("const a = <path d=\"M0\" />;\nconst b = <svg><path /></svg>;");
    assert!(code.contains(r#"_$templateSVG("<svg><path d=M0>")"#), "{code}");
    assert!(code.contains(r#"_$template("<svg><path>")"#), "{code}");
    let code = run("const a = <math><mi>{x}</mi></math>;");
    assert!(code.contains(r#"_$templateMathML("<math><mi>")"#), "{code}");
    let code = run("const a = <div><math><mi>x</mi></math></div>;");
    assert!(!code.contains("templateMathML"), "{code}");
}

#[test]
fn generated_names_avoid_names_in_the_source() {
    let code = run(
        "const _$template = 1, _el$ = 2, _$insert = 3, _p$ = 4, _v$ = 5, _$branch = 6;\n\
         export const a = <p class={c()} title={t()}>{_$template}{_el$}{_$insert}{_p$}{_v$}{x() ? <b/> : _$branch}<b/></p>;",
    );
    assert!(
        code.contains("_$template2(") && code.contains("_el$2") && code.contains("_$insert2"),
        "{code}"
    );
    assert!(
        code.contains("_p$2") && code.contains("_v$2") && code.contains("_$branch2("),
        "{code}"
    );
}

#[test]
fn string_concatenation_folds_but_number_addition_does_not() {
    let code = run(r#"const a = <div title={"a" + "b" + `c`}>{"x" + 1}</div>;"#);
    assert_eq!(templates(&code), ["<div title=abc>x1"]);
    let code = run("const a = <div title={1 + 2}>{\"x\" + y}</div>;");
    assert!(!code.contains("title="), "{code}");
    assert!(code.contains("1 + 2"), "{code}");
}

#[test]
fn runtime_imports_follow_the_leading_imports() {
    let code = run("import { a } from \"a\";\nimport b from \"b\";\nconst x = <i/>;");
    let runtime = code.find("from \"reze-js\"").unwrap();
    assert!(code.find("from \"b\"").unwrap() < runtime);
    assert!(runtime < code.find("const x").unwrap());
}

#[test]
fn delegated_events_are_registered_once_in_sorted_order() {
    let code =
        run("const a = <><b onInput={f} /><i onClick={() => g()} /><u onClick={() => h()} /></>;");
    assert_eq!(code.matches("_$delegateEvents(").count(), 1, "{code}");
    assert!(code.trim_end().ends_with(r#"_$delegateEvents(["click", "input"]);"#), "{code}");
}

#[test]
fn a_source_map_token_maps_generated_code_back_to_its_jsx() {
    let source = "const n = 1;\nconst a = <p>{n}</p>;\n";
    let out = output_for(source, "test.tsx", &Options::default());
    let map = oxc_sourcemap::SourceMap::from_json_string(out.map.as_deref().unwrap()).unwrap();
    let line = out.code.lines().position(|l| l.starts_with("const a")).unwrap() as u32;
    let token = map
        .get_tokens()
        .find(|t| t.get_dst_line() == line && t.get_dst_col() == 10)
        .expect("a token at the compiled JSX");
    assert_eq!((token.get_src_line(), token.get_src_col()), (1, 10));
}

#[test]
fn exported_signals_are_not_folded() {
    let code = run(
        "import { signal } from \"reze-js\";\nexport const [x] = signal(1);\nconst a = <p>{x()}</p>;",
    );
    assert!(code.contains("export const [x] = signal(1)"), "{code}");
    let code =
        run("import { signal } from \"reze-js\";\nconst [x] = signal(1);\nconst a = <p>{x()}</p>;");
    assert!(code.contains("const x = 1") && templates(&code) == ["<p>1"], "{code}");
}

#[test]
fn javascript_output_has_no_typescript_syntax() {
    let source = "import { signal } from \"reze-js\";\nconst [n] = signal(undefined);\nexport function A({ a = 1, ...rest }) { return <p {...rest}>{a}{n()}</p>; }";
    let out = output_for(source, "a.jsx", &Options::default());
    assert!(!out.code.contains("as any"), "{}", out.code);
}

#[test]
fn several_binds_keep_their_previous_values_in_locals() {
    let code = run("const a = <p title={t()} class={c()} style={s()}>{n() * 2}</p>;");
    assert!(code.contains("_v$ !== _p$ && ("), "{code}");
    assert!(!code.contains("_p$["), "{code}");
}

#[test]
fn intrinsics_used_as_values_are_errors() {
    for source in [
        "import { Show } from \"reze-js\"; const X = Show;",
        "import { For } from \"reze-js\"; export { For };",
        "import { Match } from \"reze-js\"; f(Match);",
        "import * as R from \"reze-js\"; const X = R.Switch;",
    ] {
        let errors = errors(source);
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, Code::ControlFlowAsValue, "{source}");
    }
    let code = run("import { Show } from \"reze-js\"; const a = <Show when={x()}><b /></Show>;");
    assert!(code.contains("_$branch("), "{code}");
    let code = run("import * as R from \"reze-js\"; const a = <R.Show when={x()}><b /></R.Show>;");
    assert!(code.contains("_$branch("), "{code}");
}

#[test]
fn a_local_function_named_like_an_intrinsic_is_a_component() {
    let code = run("function Show() { return null; }\nconst a = <Show when={x()}><b /></Show>;");
    assert!(code.contains("_$createComponent(Show,"), "{code}");
    assert!(!code.contains("branch"), "{code}");
}

#[test]
fn static_spreads_dissolve_into_one_props_object() {
    let code = run(r#"const a = <Card a={1} {...{ b: 2 }} c="x" />;"#);
    assert!(code.contains(r#"{ a: 1, b: 2, c: "x" }"#), "{code}");
    assert!(!code.contains("mergeProps"), "{code}");
    let code = run(r#"const a = <Card a={1} {...{}} />;"#);
    assert!(code.contains("{ a: 1 }"), "{code}");
    assert!(!code.contains("mergeProps"), "{code}");
}

#[test]
fn spreads_shadowed_by_maybe_undefined_stay_generic() {
    let code = run(r#"const a = <Card {...{ a: 1 }} a={value} />;"#);
    assert!(code.contains("mergeProps"), "{code}");
    let code = run(r#"const a = <Card b={2} {...{ __proto__: x }} />;"#);
    assert!(code.contains("mergeProps"), "{code}");
    let code = run(r#"const a = <Card {...dyn} {...{ a: 1 }} />;"#);
    assert!(code.contains("mergeProps"), "{code}");
    assert!(code.contains("{ a: 1 }"), "{code}");
}

#[test]
fn props_merge_of_literals_dissolves() {
    let code =
        run("import { $props } from \"reze-js\";\nconst m = $props.merge({ a: 1 }, { b: 2 });");
    assert!(code.contains("const m = { a: 1, b: 2 };"), "{code}");
    assert!(!code.contains("$props"), "{code}");
    let code = run("import { $props } from \"reze-js\";\nconst m = $props.merge();");
    assert!(code.contains("const m = {};"), "{code}");
    assert!(!code.contains("$props"), "{code}");
    let code =
        run("import { $props as P } from \"reze-js\";\nconst m = P.merge({ a: 1 }, { b: \"x\" });");
    assert!(code.contains("{ a: 1, b: \"x\" }"), "{code}");
    assert!(!code.contains("$props"), "{code}");
}

#[test]
fn props_residue_compiles_to_runtime_calls() {
    let code = run("import { $props } from \"reze-js\";\nconst r = $props.merge(a, { b: 2 });");
    assert!(code.contains("import { mergeProps }"), "{code}");
    assert!(code.contains("const r = mergeProps(a, { b: 2 });"), "{code}");
    let code = run(
        "import { $props } from \"reze-js\";\nconst [a, rest] = $props.splitByGroups(props, [\"x\"]);",
    );
    assert!(code.contains("import { splitProps }"), "{code}");
    assert!(!code.contains("$props"), "{code}");
    assert!(code.contains("splitProps(props, [\"x\"])"), "{code}");
    let code = run("import { $props } from \"reze-js\";\nconst r = $props.omit(props, \"x\");");
    assert!(code.contains("import { omitProps }"), "{code}");
    assert!(code.contains("omitProps(props, \"x\")"), "{code}");
    let code = run(
        "import { $props } from \"reze-js\";\nconst m = $props.merge({ a: 1 });\nconst r = $props.merge(a, { b: 2 });",
    );
    assert!(code.contains("const m = { a: 1 };"), "{code}");
    assert!(code.contains("mergeProps(a, { b: 2 })"), "{code}");
    let code =
        run("import { $props, mergeProps } from \"reze-js\";\nconst r = $props.merge(a, b);");
    assert!(code.contains("import { mergeProps }"), "{code}");
    assert!(!code.contains("$props"), "{code}");
    assert!(code.contains("mergeProps(a, b)"), "{code}");
}

#[test]
fn props_used_as_a_value_is_an_error() {
    for source in [
        "import { $props } from \"reze-js\";\n\nexport const view = $props;\n",
        "import { $props } from \"reze-js\";\n\nexport const pick = $props.pick;\n",
        "import { $props } from \"reze-js\";\n\nexport const m = $props.mix(a, b);\n",
    ] {
        let diagnostics = errors(source);
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code, Code::PropsAsValue, "{source}");
    }
}

#[test]
fn a_local_props_object_is_not_syntax() {
    let source = "const $props = { merge: (...a) => a };\nconst m = $props.merge({ a: 1 });";
    assert!(compile(source, "test.tsx", &Options::default()).unwrap().is_none(), "{source}");
}

#[test]
fn props_split_of_literals_dissolves() {
    let code = run(
        "import { $props } from \"reze-js\";\nconst [a, rest] = $props.splitByGroups({ x: 1, y: 2 }, [\"x\"]);",
    );
    assert!(code.contains("const [a, rest] = [{ x: 1 }, { y: 2 }];"), "{code}");
    assert!(!code.contains("$props"), "{code}");
    let code = run(
        "import { $props } from \"reze-js\";\nconst [a, b, rest] = $props.splitByGroups({ x: 1, y: 2 }, [\"y\", \"z\"], [\"x\", \"y\"]);",
    );
    assert!(code.contains("const [a, b, rest] = [{ y: 2 }, { x: 1 }, {}];"), "{code}");
    let code = run(
        "import { $props } from \"reze-js\";\nconst [a, rest] = $props.splitByGroups({ a: f() }, [\"a\"]);",
    );
    assert!(code.contains("const [a, rest] = [{ a: f() }, {}];"), "{code}");
    let code = run(
        "import { $props } from \"reze-js\";\nconst [a, rest] = $props.splitByGroups(props, keys);",
    );
    assert!(code.contains("splitProps(props, keys)"), "{code}");
    assert!(code.contains("import { splitProps }"), "{code}");
}

#[test]
fn props_omit_of_literals_dissolves() {
    let code =
        run("import { $props } from \"reze-js\";\nconst r = $props.omit({ x: 1, y: 2 }, \"x\");");
    assert!(code.contains("const r = { y: 2 };"), "{code}");
    assert!(!code.contains("$props"), "{code}");
    let code = run("import { $props } from \"reze-js\";\nconst r = $props.omit({ x: 1 }, \"x\");");
    assert!(code.contains("const r = {};"), "{code}");
    let code = run("import { $props } from \"reze-js\";\nconst r = $props.omit(props, key);");
    assert!(code.contains("omitProps(props, key)"), "{code}");
    assert!(code.contains("import { omitProps }"), "{code}");
}

#[test]
fn a_control_flow_attribute_fix_removes_it() {
    let source =
        "import { Show } from \"reze-js\";\nconst a = <Show when={x()} keyed><b /></Show>;";
    let errors = errors(source);
    assert_eq!(errors[0].code, Code::ControlFlowAttribute);
    assert_eq!(errors[0].data["attribute"], "keyed");
    let edit = &errors[0].fixes[0].edits[0];
    assert_eq!(&source[edit.start as usize..edit.end as usize], " keyed");
    let errors = self::errors(
        "import { For } from \"reze-js\";\nconst a = <For each={x()} {...rest}>{(r) => r()}</For>;",
    );
    assert_eq!(errors[0].data["attribute"], "{...}");
}

#[test]
fn hot_modules_accept_only_when_every_export_is_a_component() {
    let options = Options { hot: true, ..Options::default() };
    let source = "export function Counter() { return <b />; }";
    let code = output_for(source, "a.tsx", &options).code;
    assert!(code.contains("import.meta.hot.accept()"), "{code}");
    let code = output_for(&format!("{source}\nexport const x = 1;"), "a.tsx", &options).code;
    assert!(code.contains("_$hotComponent(") && !code.contains("accept"), "{code}");
    let code = output_for(source, "a.tsx", &Options::default()).code;
    assert!(!code.contains("hotComponent") && !code.contains("import.meta"), "{code}");
}
