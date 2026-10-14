mod common;

use common::{apply_fix, errors, output_for};
use reze_compiler::{Code, Diagnostic, Options, Severity, compile};

fn only_error(source: &str, code: Code) -> Diagnostic {
    let diagnostics = errors(source, &Options::default());
    assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
    assert_eq!(diagnostics[0].code, code, "{source}");
    assert_eq!(diagnostics[0].severity, Severity::Error, "{source}");
    diagnostics.into_iter().next().expect("one diagnostic")
}

#[test]
fn show_and_match_values_read_as_signals_and_extra_children_have_no_value_param() {
    let out = output_for(
        "import { Show } from \"reze-js\";\n\nexport const view = <Show when={user()}>{(u) => <b title={u.name} onClick={() => save(u)}>{u.name}</b>}</Show>;\n",
        &Options::default(),
    );
    assert!(out.code.contains("u().name"), "{}", out.code);
    assert!(out.code.contains("save(u())"), "{}", out.code);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);

    let out = output_for(
        "import { Match, Switch } from \"reze-js\";\n\nexport const view = <Switch><Match when={code()}>{(c) => <b>{c.name}</b>}</Match></Switch>;\n",
        &Options::default(),
    );
    assert!(out.code.contains("c().name"), "{}", out.code);

    let source = "import { Show } from \"reze-js\";\n\nexport const view = <Show when={user()}>{(u) => u()}<p>static</p></Show>;\n";
    assert!(compile(source, "test.tsx", &Options::default()).is_ok(), "{source}");
}

#[test]
fn called_flow_params_are_refused_and_the_show_fix_unwraps_them() {
    let source = "import { Show } from \"reze-js\";\n\nexport const view = <Show when={user()}>{(u) => <b>{u().name}</b>}</Show>;\n";
    let diagnostic = only_error(source, Code::FlowParamCalled);
    assert_eq!(diagnostic.data["param"], "u");
    let fixed = apply_fix(source, &diagnostic);
    assert!(fixed.contains("{u.name}"), "{fixed}");
    let out = output_for(&fixed, &Options::default());
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);

    only_error(
        "import { Match, Switch } from \"reze-js\";\n\nexport const view = <Switch><Match when={code()}>{(c) => <b>{c()}</b>}</Match></Switch>;\n",
        Code::FlowParamCalled,
    );
    only_error(
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()}>{(item, index) => <li>{index()}</li>}</For></ul>;\n",
        Code::FlowParamCalled,
    );
    only_error(
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()} keyed={false}>{(item) => <li>{item()}</li>}</For></ul>;\n",
        Code::FlowParamCalled,
    );
    for source in [
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()} keyed={(item) => item.id}>{(item) => <li>{item()}</li>}</For></ul>;\n",
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()} keyed={(item) => item.id}>{(item, index) => <li>{index()}</li>}</For></ul>;\n",
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()} keyed>{(item, index) => <li>{index()}</li>}</For></ul>;\n",
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()} keyed={true}>{(item, index) => <li>{index()}</li>}</For></ul>;\n",
    ] {
        only_error(source, Code::FlowParamCalled);
    }
    let nested = "import { Show } from \"reze-js\";\n\nexport const view = <Show when={user()}>{(u) => <b onClick={() => save(u())}>{u}</b>}</Show>;\n";
    only_error(nested, Code::FlowParamCalled);
}

#[test]
fn for_params_follow_the_keyed_mode() {
    let out = output_for(
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()}>{(item, index) => <li>{index + 0}:{item.name}</li>}</For></ul>;\n",
        &Options::default(),
    );
    assert!(out.code.contains("index()"), "{}", out.code);
    assert!(!out.code.contains("item()"), "{}", out.code);
    let raw_call = "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()}>{(item) => <li>{item()}</li>}</For></ul>;\n";
    assert!(compile(raw_call, "test.tsx", &Options::default()).is_ok(), "{raw_call}");

    let out = output_for(
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()} keyed={false}>{(item, index) => <li>{index + 0}:{item.name}</li>}</For></ul>;\n",
        &Options::default(),
    );
    assert!(out.code.contains("item()"), "{}", out.code);
    assert!(!out.code.contains("index()"), "{}", out.code);

    let out = output_for(
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()} keyed={(item) => item.id}>{(item, index) => <li>{index + 0}:{item.name}</li>}</For></ul>;\n",
        &Options::default(),
    );
    assert!(out.code.contains("item()"), "{}", out.code);
    assert!(out.code.contains("index()"), "{}", out.code);

    let out = output_for(
        "import { For } from \"reze-js\";\n\nconst keyFn = (row) => row.id;\nexport const view = <ul><For each={items()} keyed={keyFn}>{(item) => <li>{item.name}</li>}</For></ul>;\n",
        &Options::default(),
    );
    assert!(out.code.contains("item()"), "{}", out.code);
    let out = output_for(
        "import { For } from \"reze-js\";\n\nconst INDEXED = false;\nexport const view = <ul><For each={items()} keyed={INDEXED}>{(item) => <li>{item.name}</li>}</For></ul>;\n",
        &Options::default(),
    );
    assert!(out.code.contains("item()"), "{}", out.code);
}

#[test]
fn for_children_attribute_and_nested_rows_rewrite_by_symbol() {
    let out = output_for(
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()} keyed={false} children={(item) => <li>{item.name}</li>} /></ul>;\n",
        &Options::default(),
    );
    assert!(out.code.contains("item()"), "{}", out.code);

    let out = output_for(
        "import { For } from \"reze-js\";\n\nexport const view = <For each={groups()} keyed={false}>{(group) => <For each={group.items}>{(item) => <li>{group.name}:{item.name}</li>}</For>}</For>;\n",
        &Options::default(),
    );
    assert!(out.code.contains("group().name"), "{}", out.code);
    assert!(out.code.contains("group().items"), "{}", out.code);
    assert!(out.code.contains("item.name"), "{}", out.code);
    assert!(!out.code.contains("item()"), "{}", out.code);
}

#[test]
fn dynamic_keyed_and_non_inline_map_children_are_refused() {
    for keyed in
        ["keyed={mode}", "keyed={flags.keyed}", "keyed={pick()}", "keyed={ok ? false : true}"]
    {
        let source = format!(
            "import {{ For }} from \"reze-js\";\n\nexport const view = <ul><For each={{items()}} {keyed}>{{(item) => <li>{{item}}</li>}}</For></ul>;\n"
        );
        only_error(&source, Code::ForKeyedDynamic);
    }
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
fn values_that_are_not_flow_params_are_untouched() {
    let out = output_for(
        "import { For } from \"reze-js\";\n\nexport const view = <ul><For each={items()}>{({ id }) => <li>{id}</li>}</For></ul>;\n",
        &Options::default(),
    );
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);

    let out = output_for(
        "import { Show } from \"reze-js\";\n\nexport const view = <Show when={user()}>{(u) => { u = fallback; return <b>{u}</b>; }}</Show>;\n",
        &Options::default(),
    );
    assert!(out.code.contains("u = fallback"), "{}", out.code);

    let source = "export const render = (u) => u();\n";
    assert!(compile(source, "test.tsx", &Options::default()).unwrap().is_none(), "{source}");
    let source = "function Show(props) {\n  return <div>{props.children}</div>;\n}\nexport const view = <Show>{(u) => u()}</Show>;\n";
    assert!(compile(source, "test.tsx", &Options::default()).is_ok(), "{source}");
    let source = "import { Portal } from \"reze-js\";\n\nexport const view = <Portal>{(x) => x()}</Portal>;\n";
    assert!(compile(source, "test.tsx", &Options::default()).is_ok(), "{source}");
}
