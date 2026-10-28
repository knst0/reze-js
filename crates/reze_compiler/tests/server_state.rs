use reze_compiler::{Code, CompileTarget, Options, compile};

fn options(target: CompileTarget) -> Options {
    Options { target, module_id: Some("src/view.tsx".into()), ..Options::default() }
}

fn errors(source: &str, target: CompileTarget) -> Vec<Code> {
    match compile(source, "src/view.tsx", &options(target)) {
        Ok(Some(out)) => out.diagnostics.iter().map(|d| d.code).collect(),
        Ok(None) => Vec::new(),
        Err(errors) => errors.iter().map(|d| d.code).collect(),
    }
}

#[test]
fn a_component_that_writes_module_state_fails_the_html_target() {
    let source = "let cart = 0;\nexport function Cart() {\n  cart += 1;\n  return <p>{cart}</p>;\n}";
    assert_eq!(errors(source, CompileTarget::Html), [Code::ModuleStateShared]);
    assert!(errors(source, CompileTarget::Client).is_empty());
    assert!(errors(source, CompileTarget::Island).is_empty());
}

#[test]
fn a_route_callback_that_writes_module_state_fails_the_html_target() {
    let source = "let seen = 0;\nexport const route = { preload() { seen = seen + 1; return seen; } };\nexport default function Page() { return <p>page</p>; }";
    assert_eq!(errors(source, CompileTarget::Html), [Code::ModuleStateShared]);
}

#[test]
fn a_keyed_route_callback_that_writes_module_state_fails_the_html_target() {
    let source = "let seen = 0;\nexport const route = { beforeLoad: () => { seen = 1; } };\nexport default function Page() { return <p>page</p>; }";
    assert_eq!(errors(source, CompileTarget::Html), [Code::ModuleStateShared]);
}

#[test]
fn an_event_handler_write_is_not_request_time() {
    let source =
        "let clicks = 0;\nexport function Counter() {\n  return <button onClick={() => { clicks += 1; }}>{clicks}</button>;\n}";
    assert!(errors(source, CompileTarget::Html).is_empty());
}

#[test]
fn module_state_syntax_is_recreated_per_request_and_writes_it_freely() {
    let source =
        "import { signal, store } from \"reze-js\";\nlet cart = signal(0);\nlet meta = store({ hits: 0 });\nexport function Cart() {\n  cart += 1;\n  meta.hits += 1;\n  return <p>{cart}</p>;\n}";
    assert!(errors(source, CompileTarget::Html).is_empty());
}

#[test]
fn a_plain_module_write_outside_a_component_is_left_alone() {
    let source = "let build = 0;\nbuild = 1;\nexport function Page() { return <p>{build}</p>; }";
    assert!(errors(source, CompileTarget::Html).is_empty());
}
