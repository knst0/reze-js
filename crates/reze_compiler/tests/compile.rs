mod common;

use common::{errors, output_for};
use reze_compiler::{Code, CompileTarget, Options, Severity, compile};

#[test]
fn unrelated_code_is_left_alone() {
    for (source, filename) in [
        ("const a = 1 < 2;", "a.ts"),
        ("const signal = 1; export const a = signal;", "a.ts"),
        ("const mergeProps = (...a) => a;\nconst m = mergeProps({ a: 1 });", "test.tsx"),
    ] {
        assert!(compile(source, filename, &Options::default()).unwrap().is_none(), "{source}");
    }
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
        let out = output_for(&source, &Options::default());
        let map = oxc_sourcemap::SourceMap::from_json_string(out.map.as_deref().unwrap()).unwrap();
        let token = map
            .get_tokens()
            .find(|token| (token.get_src_line(), token.get_src_col()) == (1, 10))
            .expect("the JSX origin is retained");
        assert_eq!(token.get_name_id(), None, "JSX is not an original identifier");
    }
}

#[test]
fn control_flow_and_props_used_as_values_are_errors() {
    for source in [
        "import { Show } from \"reze-js\"; const X = Show;",
        "import { For } from \"reze-js\"; export { For };",
        "import { Match } from \"reze-js\"; f(Match);",
        "import * as R from \"reze-js\"; const X = R.Switch;",
    ] {
        let errors = errors(source, &Options::default());
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, Code::ControlFlowAsValue, "{source}");
    }
    for source in [
        "import { Show } from \"reze-js\"; const a = <Show when={x()}><b /></Show>;",
        "import * as R from \"reze-js\"; const a = <R.Show when={x()}><b /></R.Show>;",
    ] {
        assert!(compile(source, "test.tsx", &Options::default()).expect("compiles").is_some());
    }
    for source in [
        "import { mergeProps } from \"reze-js\";\n\nexport const view = mergeProps;\n",
        "import { splitProps } from \"reze-js\";\n\nexport const pick = splitProps.extra;\n",
        "import { omitProps } from \"reze-js\";\n\nexport const m = omitProps.unknown(a, b);\n",
    ] {
        let diagnostics = errors(source, &Options::default());
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code, Code::PropsAsValue, "{source}");
    }
}

#[test]
fn a_control_flow_attribute_fix_removes_it() {
    let source =
        "import { Show } from \"reze-js\";\nconst a = <Show when={x()} keyed><b /></Show>;";
    let found = errors(source, &Options::default());
    assert_eq!(found[0].code, Code::ControlFlowAttribute);
    assert_eq!(found[0].data["attribute"], "keyed");
    let edit = &found[0].fixes[0].edits[0];
    assert_eq!(&source[edit.start as usize..edit.end as usize], " keyed");
    let found = errors(
        "import { For } from \"reze-js\";\nconst a = <For each={x()} {...rest}>{(r) => r()}</For>;",
        &Options::default(),
    );
    assert_eq!(found[0].data["attribute"], "{...}");
}

#[test]
fn async_component_shapes_are_refused_with_their_code_and_reason() {
    for (source, reason) in [
        (
            "export async function Card(props) {\n  const id = props.id;\n  let user = await fetchUser(id);\n  user = normalize(user);\n  return <p>{user.name}</p>;\n}",
            "value-reassigned",
        ),
        (
            "export async function Card(props) {\n  const id = props.id;\n  const user = await fetchUser(id);\n  log(user);\n  const posts = await fetchPosts(user.id);\n  return <p>{posts.length}</p>;\n}",
            "statement-between-awaits",
        ),
    ] {
        let errs = errors(source, &Options::default());
        assert_eq!(errs.len(), 1, "{source}");
        assert_eq!(errs[0].code, Code::AsyncComponentShape, "{source}");
        assert_eq!(errs[0].severity, Severity::Error, "{source}");
        assert_eq!(errs[0].data["reason"], reason, "{source}");
    }
    for (source, code) in [
        (
            "export async function Card(props) {\n  const user = await fetchUser(props.id);\n  return <p>{user.name}</p>;\n}",
            Code::AsyncPropsReadInAwait,
        ),
        (
            "export async function Card({ id }) {\n  const user = await fetchUser(id);\n  return <p>{user.name}</p>;\n}",
            Code::AsyncPropsReadInAwait,
        ),
        (
            "export async function Card(props) {\n  const id = props.id;\n  try {\n    const user = await fetchUser(id);\n    return <p>{user.name}</p>;\n  } catch (error) {\n    return <p>Failed</p>;\n  }\n}",
            Code::AsyncTryAroundAwait,
        ),
        (
            "import { abortSignal } from \"reze-js\";\n\nexport async function Card(props) {\n  const id = props.id;\n  const user = await fetchUser(id);\n  const signal = abortSignal();\n  return <p>{user.name}</p>;\n}",
            Code::AbortSignalOutsideLoad,
        ),
        (
            "import { abortSignal } from \"reze-js\";\n\nexport function Card() {\n  const signal = abortSignal();\n  return <p>x</p>;\n}",
            Code::AbortSignalOutsideLoad,
        ),
        (
            "import { isPending } from \"reze-js\";\n\nexport async function Card(props) {\n  const id = props.id;\n  const busy = isPending();\n  const user = await fetchUser(id);\n  return <p>{busy ? \"Refreshing\" : user.name}</p>;\n}",
            Code::IsPendingOutsideView,
        ),
    ] {
        let errs: Vec<_> = errors(source, &Options::default())
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert_eq!(errs.len(), 1, "{source}");
        assert_eq!(errs[0].code, code, "{source}");
        assert_eq!(errs[0].severity, Severity::Error, "{source}");
    }
}

#[test]
fn async_loads_lower_to_the_load_context_and_the_view() {
    let hoisted = "export async function Card(props) {\n  const id = props.id;\n  const user = await fetchUser(id);\n  return <p>{user.name}</p>;\n}";
    assert!(compile(hoisted, "test.tsx", &Options::default()).is_ok(), "{hoisted}");
    let source = "import { abortSignal, isPending } from \"reze-js\";\n\nexport async function Card(props) {\n  const id = props.id;\n  const signal = abortSignal();\n  const user = await fetchUser(id, signal);\n  return <p>{isPending() ? \"Refreshing\" : user.name}</p>;\n}";
    let out = output_for(source, &Options::default());
    assert!(out.code.contains(".abortSignal()"), "{}", out.code);
    assert!(!out.code.contains(" abortSignal()"), "{}", out.code);
    assert!(!out.code.contains("isPending"), "{}", out.code);
    assert!(out.diagnostics.iter().all(|d| d.severity != Severity::Error), "{:?}", out.diagnostics);
}

#[test]
fn attribute_and_flow_warnings_report_their_codes() {
    let out = output_for(
        "import { For } from \"reze-js\";\nimport { signal } from \"reze-js\";\nlet n = signal(0);\nexport function bump() { n = 1; }\nfunction Greeting({ name = fallback() }) {\n  return <p key=\"k\" clas=\"x\" title={n} title=\"y\">{name}<For each={[1, 2]}>{(i) => i}</For></p>;\n}",
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
    output_for(&source, &Options::default()).code
}

#[test]
fn binding_fixing_follows_the_reads_and_matches_island_output() {
    for attributes in [r#"title={a + "-" + b}"#, r#"class={a > 1 ? "x" : "y"}"#] {
        let code = client_code_with_signals(attributes);
        assert!(code.contains("fixedRenderEffect("), "{attributes}\n{code}");
        assert!(!code.contains("_$renderEffect("), "{attributes}\n{code}");
    }
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
    let code = client_code_with_signals(r#"title={a} data-x={a > 0 && a}"#);
    assert!(code.contains("_$renderEffect("), "{code}");
    assert!(!code.contains("fixedRenderEffect("), "{code}");
    let source = "import { signal } from \"reze-js\";\nexport function C() {\n  let a = signal(0);\n  let b = signal(0);\n  return <div title={a + \"-\" + b} onClick={() => { a += 1; b += 1; }} />;\n}";
    let options = Options {
        target: CompileTarget::Island,
        module_id: Some("m".into()),
        ..Options::default()
    };
    let client = output_for(source, &Options::default()).code;
    let island = output_for(source, &options).code;
    assert_eq!(
        island.matches("fixedRenderEffect").count(),
        client.matches("fixedRenderEffect").count(),
        "{island}"
    );
}

#[test]
fn handlers_use_static_delegation_only_when_provably_local() {
    for source in [
        "function handle() {}\nexport function C() {\n  return <button onClick={handle}>x</button>;\n}",
        "const handle = () => {};\nexport function C() {\n  return <button onClick={handle}>x</button>;\n}",
        "export function C() {\n  function handle() {}\n  return <button onClick={handle}>x</button>;\n}",
        "export function C() {\n  const handle = () => {};\n  const alias = handle;\n  return <button onClick={alias}>x</button>;\n}",
    ] {
        let code = output_for(source, &Options::default()).code;
        assert!(code.contains("$$click ="), "{source}\n{code}");
        assert!(!code.contains("addEventListener"), "{source}\n{code}");
    }
    for source in [
        "export function C(props) {\n  return <button onClick={props.handle}>x</button>;\n}",
        "export function C(handle) {\n  return <button onClick={handle}>x</button>;\n}",
        "export function C() {\n  let handle = () => {};\n  return <button onClick={handle}>x</button>;\n}",
        "export function C() {\n  function handle() {}\n  handle = () => {};\n  return <button onClick={handle}>x</button>;\n}",
        "export function handle() {}\nexport function C() {\n  return <button onClick={handle}>x</button>;\n}",
    ] {
        let code = output_for(source, &Options::default()).code;
        assert!(code.contains("addEventListener"), "{source}\n{code}");
        assert!(!code.contains("$$click ="), "{source}\n{code}");
    }
}

#[test]
fn refs_to_immutable_bindings_never_assign_them() {
    let source = "import { elementRef } from \"./refs\";\nfunction Field(props) {\n  return <input ref={props.ref} />;\n}\nexport function C(props) {\n  const merged = mergeRefs(props.ref, elementRef);\n  const handle = (el) => props.seen(el);\n  let slot;\n  return <div ref={merged}><b ref={handle} /><i ref={elementRef} /><u ref={slot} /><Field ref={merged} /><Field ref={handle} /><Field ref={elementRef} /><Field ref={slot} /></div>;\n}";
    for target in [CompileTarget::Client, CompileTarget::Island] {
        let options = Options { target, module_id: Some("m".into()), ..Options::default() };
        let code = output_for(source, &options).code;
        for binding in ["merged", "handle", "elementRef"] {
            assert!(!code.contains(&format!(": {binding} = ")), "{binding}\n{code}");
        }
        assert!(code.contains("_$use(handle, "), "{code}");
        assert!(code.contains("typeof merged === \"function\" && _$use(merged, "), "{code}");
        assert!(code.contains("\"ref\": handle"), "{code}");
        assert!(code.contains("slot = "), "{code}");
    }
}

#[test]
fn closed_spreads_are_fixed_only_when_every_entry_is() {
    let code = client_body(
        "let a = signal(0); let b = signal(0);",
        r#"<><div {...{ title: a + "-" + b }} /><button onClick={() => { a += 1; b += 1; }}>x</button></>"#,
    );
    assert!(code.contains("fixedRenderEffect("), "{code}");
    assert!(!code.contains("_$renderEffect("), "{code}");
    let code = client_body(
        "let a = signal(0); let b = signal(0);",
        r#"<><div {...{ title: a > 0 ? b + "x" : "y" }} /><button onClick={() => { a += 1; b += 1; }}>x</button></>"#,
    );
    assert!(code.contains("_$renderEffect("), "{code}");
    assert!(!code.contains("fixedRenderEffect("), "{code}");
}

fn client_body(declarations: &str, view: &str) -> String {
    let source = format!(
        "import {{ signal }} from \"reze-js\";\nexport function C(props) {{\n  {declarations}\n  return {view};\n}}"
    );
    output_for(&source, &Options::default()).code
}

#[test]
fn child_reads_are_text_writes_only_while_every_write_keeps_the_kind() {
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
fn runtime_imports_and_props_calls_are_lowered_to_the_internal_runtime() {
    let source =
        "import { effect } from \"reze-js\";\nexport const f = () => { effect(() => {}); };";
    let out = output_for(source, &Options::default());
    assert!(out.code.contains("from \"reze-js/internal/runtime\""), "{}", out.code);
    assert!(!out.code.contains("from \"reze-js\""), "{}", out.code);

    let source = "export { effect, render, type Getter } from \"reze-js\";";
    let out = output_for(source, &Options::default());
    assert!(out.code.contains("effect } from \"reze-js/internal/runtime\""), "{}", out.code);
    assert!(out.code.contains("render"), "{}", out.code);
    assert!(out.code.contains("from \"reze-js\""), "{}", out.code);
    let only = output_for("export { effect } from \"reze-js\";", &Options::default());
    assert!(!only.code.contains("from \"reze-js\""), "{}", only.code);

    let source = "import * as R from \"reze-js\";\nexport const f = () => { R.effect(() => {}); };";
    let out = output_for(source, &Options::default());
    assert!(out.code.contains("from \"reze-js/internal/runtime\""), "{}", out.code);
    assert!(!out.code.contains("R.effect"), "{}", out.code);

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
        let out = output_for(source, &Options::default());
        assert!(out.code.contains(callee), "{source}\n{}", out.code);
        assert!(out.code.contains("from \"reze-js/internal/runtime\""), "{source}\n{}", out.code);
        assert!(!out.code.contains("from \"reze-js\""), "{source}\n{}", out.code);
    }
}
