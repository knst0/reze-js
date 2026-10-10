use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use reze_compiler::{Code, Diagnostic, Options, Output, Severity, compile};

fn output_for(source: &str) -> Output {
    let out =
        compile(source, "test.tsx", &Options::default()).expect("compiles").expect("rewrites");
    assert_valid(&out.code);
    out
}

fn errors(source: &str) -> Vec<Diagnostic> {
    compile(source, "test.tsx", &Options::default()).err().expect("fails")
}

fn assert_valid(code: &str) {
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let parsed = Parser::new(&allocator, code, source_type).parse();
    assert!(parsed.diagnostics.is_empty(), "{:?}\n{code}", parsed.diagnostics);
    let semantic = SemanticBuilder::new().with_check_syntax_error(true).build(&parsed.program);
    assert!(semantic.diagnostics.is_empty(), "{:?}\n{code}", semantic.diagnostics);
}

fn only_error(source: &str, code: Code) -> Diagnostic {
    let diagnostics = errors(source);
    assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
    assert_eq!(diagnostics[0].code, code, "{source}");
    assert_eq!(diagnostics[0].severity, Severity::Error, "{source}");
    diagnostics.into_iter().next().expect("one diagnostic")
}

fn apply_fix(source: &str, diagnostic: &Diagnostic) -> String {
    let mut edits: Vec<_> = diagnostic.fixes[0].edits.iter().collect();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.start));
    let mut fixed = source.to_string();
    for edit in edits {
        fixed.replace_range(edit.start as usize..edit.end as usize, &edit.text);
    }
    fixed
}

#[test]
fn show_value_reads_as_signal() {
    let out = output_for(
        "import { Show } from \"reze-js\";\n\nexport const view = <Show when={user()}>{(u) => <b title={u.name} onClick={() => save(u)}>{u.name}</b>}</Show>;\n",
    );
    assert!(out.code.contains("u().name"), "{}", out.code);
    assert!(out.code.contains("save(u())"), "{}", out.code);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn show_value_called_is_an_error_with_a_fix() {
    let source = "import { Show } from \"reze-js\";\n\nexport const view = <Show when={user()}>{(u) => <b>{u().name}</b>}</Show>;\n";
    let diagnostic = only_error(source, Code::FlowParamCalled);
    assert_eq!(diagnostic.data["param"], "u");
    let fixed = apply_fix(source, &diagnostic);
    assert!(fixed.contains("{u.name}"), "{fixed}");
    let out = output_for(&fixed);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn match_value_reads_as_signal() {
    let out = output_for(
        "import { Match, Switch } from \"reze-js\";\n\nexport const view = <Switch><Match when={code()}>{(c) => <b>{c.name}</b>}</Match></Switch>;\n",
    );
    assert!(out.code.contains("c().name"), "{}", out.code);
    let source = "import { Match, Switch } from \"reze-js\";\n\nexport const view = <Switch><Match when={code()}>{(c) => <b>{c()}</b>}</Match></Switch>;\n";
    only_error(source, Code::FlowParamCalled);
}

#[test]
fn for_identity_index_is_a_getter_item_is_raw() {
    let out = output_for(
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()}>{(item, index) => <li>{index + 0}:{item.name}</li>}</For></ul>;\n",
    );
    assert!(out.code.contains("index()"), "{}", out.code);
    assert!(!out.code.contains("item()"), "{}", out.code);
    let source = "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()}>{(item, index) => <li>{index()}</li>}</For></ul>;\n";
    only_error(source, Code::FlowParamCalled);
    let raw_call = "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()}>{(item) => <li>{item()}</li>}</For></ul>;\n";
    assert!(compile(raw_call, "test.tsx", &Options::default()).is_ok(), "{raw_call}");
}

#[test]
fn for_indexed_item_is_a_getter_index_is_raw() {
    let out = output_for(
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()} keyed={false}>{(item, index) => <li>{index + 0}:{item.name}</li>}</For></ul>;\n",
    );
    assert!(out.code.contains("item()"), "{}", out.code);
    assert!(!out.code.contains("index()"), "{}", out.code);
    let source = "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()} keyed={false}>{(item) => <li>{item()}</li>}</For></ul>;\n";
    only_error(source, Code::FlowParamCalled);
}

#[test]
fn for_keyed_function_makes_both_params_getters() {
    let out = output_for(
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()} keyed={(item) => item.id}>{(item, index) => <li>{index + 0}:{item.name}</li>}</For></ul>;\n",
    );
    assert!(out.code.contains("item()"), "{}", out.code);
    assert!(out.code.contains("index()"), "{}", out.code);
    for source in [
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()} keyed={(item) => item.id}>{(item) => <li>{item()}</li>}</For></ul>;\n",
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()} keyed={(item) => item.id}>{(item, index) => <li>{index()}</li>}</For></ul>;\n",
    ] {
        only_error(source, Code::FlowParamCalled);
    }
}

#[test]
fn for_keyed_true_and_bare_keyed_are_identity() {
    for keyed in ["keyed", "keyed={true}"] {
        let source = format!(
            "import {{ For }} from \"reze-js\";\n\nexport const view = <ul><For each={{items()}} {keyed}>{{(item, index) => <li>{{index()}}</li>}}</For></ul>;\n"
        );
        only_error(&source, Code::FlowParamCalled);
    }
}

#[test]
fn for_keyed_named_values_resolve() {
    let out = output_for(
        "import { For } from \"reze-js\";\n\nconst keyFn = (row) => row.id;\nexport const view = <ul><For each={items()} keyed={keyFn}>{(item) => <li>{item.name}</li>}</For></ul>;\n",
    );
    assert!(out.code.contains("item()"), "{}", out.code);
    let out = output_for(
        "import { For } from \"reze-js\";\n\nconst INDEXED = false;\nexport const view = <ul><For each={items()} keyed={INDEXED}>{(item) => <li>{item.name}</li>}</For></ul>;\n",
    );
    assert!(out.code.contains("item()"), "{}", out.code);
}

#[test]
fn for_dynamic_keyed_is_an_error() {
    for keyed in
        ["keyed={mode}", "keyed={flags.keyed}", "keyed={pick()}", "keyed={ok ? false : true}"]
    {
        let source = format!(
            "import {{ For }} from \"reze-js\";\n\nexport const view = <ul><For each={{items()}} {keyed}>{{(item) => <li>{{item}}</li>}}</For></ul>;\n"
        );
        only_error(&source, Code::ForKeyedDynamic);
    }
}

#[test]
fn for_map_must_be_an_inline_function() {
    for map in ["{row}", "{rows[row]}", "{makeRow()}"] {
        let source = format!(
            "import {{ For }} from \"reze-js\";\n\nexport const view = <ul><For each={{items()}}>{map}</For></ul>;\n"
        );
        let diagnostic = only_error(&source, Code::ControlFlowChildren);
        assert_eq!(diagnostic.data["tag"], "For");
    }
    let source = "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()} children={row} /></ul>;\n";
    only_error(source, Code::ControlFlowChildren);
}

#[test]
fn for_children_attribute_function_rewrites() {
    let out = output_for(
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()} keyed={false} children={(item) => <li>{item.name}</li>} /></ul>;\n",
    );
    assert!(out.code.contains("item()"), "{}", out.code);
}

#[test]
fn nested_rows_rewrite_by_symbol() {
    let out = output_for(
        "import { For } from \"reze-js\";\n\nexport const view = <For each={groups()} keyed={false}>{(group) => <For each={group.items}>{(item) => <li>{group.name}:{item.name}</li>}</For>}</For>;\n",
    );
    assert!(out.code.contains("group().name"), "{}", out.code);
    assert!(out.code.contains("group().items"), "{}", out.code);
    assert!(out.code.contains("item.name"), "{}", out.code);
    assert!(!out.code.contains("item()"), "{}", out.code);
}

#[test]
fn show_with_several_children_has_no_value_param() {
    let source = "import { Show } from \"reze-js\";\n\nexport const view = <Show when={user()}>{(u) => u()}<p>static</p></Show>;\n";
    assert!(compile(source, "test.tsx", &Options::default()).is_ok(), "{source}");
}

#[test]
fn calls_in_nested_handlers_are_diagnosed() {
    let source = "import { Show } from \"reze-js\";\n\nexport const view = <Show when={user()}>{(u) => <b onClick={() => save(u())}>{u}</b>}</Show>;\n";
    only_error(source, Code::FlowParamCalled);
}

#[test]
fn destructured_identity_item_is_untouched() {
    let out = output_for(
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()}>{({ id }) => <li>{id}</li>}</For></ul>;\n",
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn writes_to_flow_params_are_untouched() {
    let out = output_for(
        "import { Show } from \"reze-js\";\n\nexport const view = <Show when={user()}>{(u) => { u = fallback; return <b>{u}</b>; }}</Show>;\n",
    );
    assert!(out.code.contains("u = fallback"), "{}", out.code);
}

#[test]
fn calls_outside_flow_functions_are_untouched() {
    let source = "export const render = (u) => u();\n";
    assert!(compile(source, "test.tsx", &Options::default()).unwrap().is_none(), "{source}");
    let source = "function Show(props) {\n  return <div>{props.children}</div>;\n}\nexport const view = <Show>{(u) => u()}</Show>;\n";
    assert!(compile(source, "test.tsx", &Options::default()).is_ok(), "{source}");
    let source = "import { Portal } from \"reze-js\";\n\nexport const view = <Portal>{(x) => x()}</Portal>;\n";
    assert!(compile(source, "test.tsx", &Options::default()).is_ok(), "{source}");
}
