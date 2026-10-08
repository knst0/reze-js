use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use reze_compiler::{Code, Diagnostic, Options, Output, Severity, compile};

fn output_for(source: &str, filename: &str, options: &Options) -> Output {
    let out = compile(source, filename, options).expect("compiles").expect("rewrites");
    assert_valid(&out.code, SourceType::from_path(filename).unwrap());
    out
}

fn errors(source: &str) -> Vec<Diagnostic> {
    compile(source, "test.tsx", &Options::default()).err().expect("fails")
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
fn jsx_source_locations_do_not_become_identifier_names() {
    for jsx in ["<p />", "<p>{n}</p>"] {
        let source = format!("const n = 1;\nconst a = {jsx};\n");
        let out = output_for(&source, "test.tsx", &Options::default());
        let map = oxc_sourcemap::SourceMap::from_json_string(out.map.as_deref().unwrap()).unwrap();
        let token = map
            .get_tokens()
            .find(|token| (token.get_src_line(), token.get_src_col()) == (1, 10))
            .expect("the JSX origin is retained");
        assert_eq!(token.get_name_id(), None, "JSX is not an original identifier");
    }
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
    for source in [
        "import { Show } from \"reze-js\"; const a = <Show when={x()}><b /></Show>;",
        "import * as R from \"reze-js\"; const a = <R.Show when={x()}><b /></R.Show>;",
    ] {
        assert!(compile(source, "test.tsx", &Options::default()).expect("compiles").is_some());
    }
}

#[test]
fn props_used_as_a_value_is_an_error() {
    for source in [
        "import { mergeProps } from \"reze-js\";\n\nexport const view = mergeProps;\n",
        "import { splitProps } from \"reze-js\";\n\nexport const pick = splitProps.extra;\n",
        "import { omitProps } from \"reze-js\";\n\nexport const m = omitProps.unknown(a, b);\n",
    ] {
        let diagnostics = errors(source);
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code, Code::PropsAsValue, "{source}");
    }
}

#[test]
fn a_local_props_object_is_not_syntax() {
    let source = "const mergeProps = (...a) => a;\nconst m = mergeProps({ a: 1 });";
    assert!(compile(source, "test.tsx", &Options::default()).unwrap().is_none(), "{source}");
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
fn unsupported_async_shapes_error_with_their_reason() {
    for (source, reason) in [
        (
            "export async function Card(props) {\n  let user = await fetchUser(props.id);\n  user = normalize(user);\n  return <p>{user.name}</p>;\n}",
            "value-reassigned",
        ),
        (
            "export async function Card(props) {\n  const user = await fetchUser(props.id);\n  log(user);\n  const posts = await fetchPosts(user.id);\n  return <p>{posts.length}</p>;\n}",
            "statement-between-awaits",
        ),
    ] {
        let errs = errors(source);
        assert_eq!(errs.len(), 1, "{source}");
        assert_eq!(errs[0].code, Code::AsyncComponentShape, "{source}");
        assert_eq!(errs[0].severity, Severity::Error, "{source}");
        assert_eq!(errs[0].data["reason"], reason, "{source}");
    }
}

#[test]
fn attribute_and_flow_warnings_report_their_codes() {
    let out = output_for(
        "import { For } from \"reze-js\";\nimport { signal } from \"reze-js\";\nlet n = signal(0);\nexport function bump() { n = 1; }\nfunction Greeting({ name = fallback() }) {\n  return <p key=\"k\" clas=\"x\" title={n} title=\"y\">{name}<For each={[1, 2]}>{(i) => i}</For></p>;\n}",
        "test.tsx",
        &Options::default(),
    );
    assert_eq!(out.diagnostics.len(), 5);
    assert_eq!(out.diagnostics[0].code, Code::PropsDestructured);
    assert_eq!(out.diagnostics[1].code, Code::KeyOnElement);
    assert_eq!(out.diagnostics[2].code, Code::UnknownAttribute);
    assert_eq!(out.diagnostics[3].code, Code::DuplicateAttribute);
    assert_eq!(out.diagnostics[4].code, Code::InlineEach);
    assert!(out.diagnostics.iter().all(|d| d.severity == Severity::Warn));
}

fn client_code_with_signals(attributes: &str) -> String {
    let source = format!(
        "import {{ signal }} from \"reze-js\";\nexport function C() {{\n  let a = signal(0);\n  let b = signal(0);\n  return <div {attributes} onClick={{() => {{ a += 1; b += 1; }}}} />;\n}}"
    );
    output_for(&source, "test.tsx", &Options::default()).code
}

#[test]
fn bindings_with_unconditional_reads_are_fixed() {
    for attributes in [r#"title={a + "-" + b}"#, r#"class={a > 1 ? "x" : "y"}"#] {
        let code = client_code_with_signals(attributes);
        assert!(code.contains("fixedRenderEffect("), "{attributes}\n{code}");
        assert!(!code.contains("_$renderEffect("), "{attributes}\n{code}");
    }
}

#[test]
fn bindings_with_conditional_or_opaque_reads_stay_dynamic() {
    for attributes in [
        r#"title={a > 1 ? b : "y"}"#,
        r#"title={a.toString()}"#,
        r#"title={a ?? b}"#,
        r#"title={a > 0 && b}"#,
    ] {
        let code = client_code_with_signals(attributes);
        assert!(code.contains("_$renderEffect("), "{attributes}\n{code}");
        assert!(!code.contains("fixedRenderEffect("), "{attributes}\n{code}");
    }
}

#[test]
fn a_group_is_fixed_only_when_every_member_is() {
    let code = client_code_with_signals(r#"title={a} data-x={a > 0 && a}"#);
    assert!(code.contains("_$renderEffect("), "{code}");
    assert!(!code.contains("fixedRenderEffect("), "{code}");
}

#[test]
fn hydrate_bindings_are_never_fixed() {
    let source = "import { signal } from \"reze-js\";\nexport function C() {\n  let a = signal(0);\n  let b = signal(0);\n  return <div title={a + \"-\" + b} onClick={() => { a += 1; b += 1; }} />;\n}";
    let options = Options {
        target: reze_compiler::CompileTarget::Hydrate,
        module_id: Some("m".into()),
        ..Options::default()
    };
    let code = output_for(source, "test.tsx", &options).code;
    assert!(!code.contains("fixedRenderEffect"), "{code}");
}

fn client_body(declarations: &str, view: &str) -> String {
    let source = format!(
        "import {{ signal }} from \"reze-js\";\nexport function C(props) {{\n  {declarations}\n  return {view};\n}}"
    );
    output_for(&source, "test.tsx", &Options::default()).code
}

#[test]
fn a_child_read_of_a_signal_whose_every_write_keeps_its_kind_is_a_text_write() {
    for (declarations, view) in [
        ("let n = signal(0);", "<p onClick={() => { n += 1; }}>{n}</p>"),
        ("let n = signal(0);", "<p onClick={() => { n++; }}>{n}</p>"),
        (r#"let s = signal("a");"#, r#"<p onClick={() => { s = s + "!"; }}>{s}</p>"#),
        (
            "let n = signal(0); let m = signal(1);",
            "<p onClick={() => { n = m; m += n; }}>{n}{m}</p>",
        ),
    ] {
        let code = client_body(declarations, view);
        assert!(code.contains(".data ="), "{declarations} {view}\n{code}");
        assert!(!code.contains("_$insert("), "{declarations} {view}\n{code}");
    }
}

#[test]
fn a_child_read_stays_an_insert_when_a_write_may_change_its_kind() {
    for (declarations, view) in [
        ("let n = signal(0);", "<p onClick={() => { n += props.step; }}>{n}</p>"),
        ("let n = signal(0);", "<p onClick={() => { n = props.value; }}>{n}</p>"),
        ("let n = signal(0);", "<p onClick={() => { n &&= props.x; }}>{n}</p>"),
        (
            "let n = signal(0); let m = signal(0);",
            "<p onClick={() => { n = m; m = props.x; }}>{n}</p>",
        ),
    ] {
        let code = client_body(declarations, view);
        assert!(code.contains("_$insert("), "{declarations} {view}\n{code}");
        assert!(!code.contains(".data ="), "{declarations} {view}\n{code}");
    }
}

#[test]
fn a_declared_plain_import_is_rewritten_to_the_runtime() {
    let source =
        "import { effect } from \"reze-js\";\nexport const f = () => { effect(() => {}); };";
    let out = output_for(source, "test.tsx", &Options::default());
    assert!(out.code.contains("from \"reze-js/internal/runtime\""), "{}", out.code);
    assert!(!out.code.contains("from \"reze-js\""), "{}", out.code);
}

#[test]
fn a_declared_reexport_is_rewritten_to_the_runtime_and_keeps_real_exports() {
    let source = "export { effect, render, type Getter } from \"reze-js\";";
    let out = output_for(source, "test.tsx", &Options::default());
    assert!(out.code.contains("effect } from \"reze-js/internal/runtime\""), "{}", out.code);
    assert!(out.code.contains("render"), "{}", out.code);
    assert!(out.code.contains("from \"reze-js\""), "{}", out.code);
    let only = output_for("export { effect } from \"reze-js\";", "test.tsx", &Options::default());
    assert!(!only.code.contains("from \"reze-js\""), "{}", only.code);
}

#[test]
fn a_namespace_member_call_is_lowered_to_a_named_runtime_import() {
    let source = "import * as R from \"reze-js\";\nexport const f = () => { R.effect(() => {}); };";
    let out = output_for(source, "test.tsx", &Options::default());
    assert!(out.code.contains("from \"reze-js/internal/runtime\""), "{}", out.code);
    assert!(!out.code.contains("R.effect"), "{}", out.code);
}

#[test]
fn props_calls_are_lowered_to_the_runtime() {
    for (source, callee) in [
        (
            "import { mergeProps } from \"reze-js\";\nexport function C(props) {\n  const m = mergeProps({ a: 1 }, props);\n  return <p>{m.a}</p>;\n}",
            "_$mergeProps(",
        ),
        (
            "import { splitProps } from \"reze-js\";\nexport function C(props) {\n  const [own, rest] = splitProps(props, [\"x\"], [\"y\"]);\n  return <p>{own.x}</p>;\n}",
            "_$splitProps(",
        ),
        (
            "import { omitProps } from \"reze-js\";\nexport function C(props) {\n  const m = omitProps(props, \"id\");\n  return <p>{m.id}</p>;\n}",
            "_$omitProps(",
        ),
    ] {
        let out = output_for(source, "test.tsx", &Options::default());
        assert!(out.code.contains(callee), "{source}\n{}", out.code);
        assert!(out.code.contains("from \"reze-js/internal/runtime\""), "{source}\n{}", out.code);
        assert!(!out.code.contains("from \"reze-js\""), "{source}\n{}", out.code);
    }
}
