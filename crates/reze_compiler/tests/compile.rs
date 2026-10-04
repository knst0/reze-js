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
fn unsupported_async_shapes_warn_with_their_reason() {
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
        let out = output_for(source, "test.tsx", &Options::default());
        assert_eq!(out.diagnostics.len(), 1, "{source}");
        assert_eq!(out.diagnostics[0].code, Code::AsyncComponentShape, "{source}");
        assert_eq!(out.diagnostics[0].severity, Severity::Warn, "{source}");
        assert_eq!(out.diagnostics[0].data["reason"], reason, "{source}");
    }
}

#[test]
fn attribute_and_flow_warnings_report_their_codes() {
    let out = output_for(
        "import { For } from \"reze-js\";\nimport { signal } from \"@rezejs/signals\";\nconst [n, setN] = signal(0); setN(1);\nfunction Greeting({ name = fallback() }) {\n  return <p key=\"k\" clas=\"x\" title={n} title=\"y\">{name}<For each={[1, 2]}>{(i) => i}</For></p>;\n}",
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
