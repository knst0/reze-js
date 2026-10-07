use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use reze_compiler::{Code, Diagnostic, Options, Severity, compile};

fn options() -> Options {
    Options { source_map: false, ..Options::default() }
}

fn output(source: &str, options: &Options) -> (String, Vec<Diagnostic>) {
    let (code, diagnostics) = match compile(source, "test.tsx", options).expect("compiles") {
        Some(out) => (out.code, out.diagnostics),
        None => (source.to_string(), Vec::new()),
    };
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &code, SourceType::tsx()).parse();
    assert!(parsed.diagnostics.is_empty(), "{:?}\n{code}", parsed.diagnostics);
    let semantic = SemanticBuilder::new().with_check_syntax_error(true).build(&parsed.program);
    assert!(semantic.diagnostics.is_empty(), "{:?}\n{code}", semantic.diagnostics);
    (code, diagnostics)
}

fn compiled(source: &str, options: &Options) -> String {
    output(source, options).0
}

fn only(source: &str, code: Code) -> Diagnostic {
    let found: Vec<_> = match compile(source, "test.tsx", &options()) {
        Ok(out) => out.map(|out| out.diagnostics).unwrap_or_default(),
        Err(errors) => errors,
    }
    .into_iter()
    .filter(|d| d.code == code)
    .collect();
    assert_eq!(found.len(), 1, "{source}\n{found:?}");
    found.into_iter().next().unwrap()
}

fn module(body: &str) -> String {
    format!("import {{ action }} from \"reze-js\";\n{body}")
}

#[test]
fn an_argument_that_is_not_a_function_literal_is_refused() {
    for argument in ["save", "make()", "", "...handlers"] {
        let source = module(&format!("export const a = action({argument});"));
        let d = only(&source, Code::ActionArgument);
        assert_eq!(d.severity, Severity::Error);
    }
    let d = only(&module("export const a = action(save);"), Code::ActionArgument);
    assert_eq!((d.start.line, d.start.column, d.end.column), (2, 24, 28));
}

#[test]
fn suspensions_the_compiler_cannot_resume_are_refused_but_nested_ones_are_not() {
    for (body, construct) in [
        ("async (s) => { for await (const x of s) use(x); }", "for await"),
        ("async () => { await using r = open(); }", "await using"),
        ("function* () { yield 1; }", "generator"),
        ("async function* () {}", "generator"),
    ] {
        let d =
            only(&module(&format!("export const a = action({body});")), Code::ActionUnsupported);
        assert_eq!(d.data["construct"], construct, "{body}");
    }
    let nested = module(
        "export const a = action(async (s) => { await drain(async () => { for await (const x of s) use(x); }); });",
    );
    compiled(&nested, &options());
}

#[test]
fn syntax_used_as_a_value_is_not_called() {
    only(&module("export const make = action;"), Code::ActionNotCalled);
    only(&module("register(action);"), Code::ActionNotCalled);
    only("import * as R from \"reze-js\";\nexport const make = R.action;", Code::ActionNotCalled);
}

#[test]
fn a_member_write_in_a_function_that_runs_later_warns_and_one_in_a_sync_callback_does_not() {
    for (nested, via) in [
        ("p.then((v) => { t.v = v; });", "a `.then` callback"),
        ("p.catch(function () { t.failed = true; });", "a `.catch` callback"),
        ("setTimeout(() => t.n++);", "a `setTimeout` callback"),
        ("queueMicrotask(() => { delete t.draft; });", "a `queueMicrotask` callback"),
        ("list.map(async (x) => { x.saved = true; });", "an async function"),
        ("p.then(() => { list.forEach((x) => { x.v = 1; }); });", "a `.then` callback"),
    ] {
        let source =
            module(&format!("export const a = action(async (t, p, list) => {{ {nested} }});"));
        let d = only(&source, Code::ActionNestedWrite);
        assert_eq!((d.severity, d.data["via"].as_str()), (Severity::Warn, via), "{nested}");
    }
    for fine in [
        "list.forEach((x) => { x.v = 1; });",
        "t.v = await p;",
        "let n = 0; setTimeout(() => { n = 1; });",
        "const inner = action(async () => { await p; t.v = 1; });",
    ] {
        let source =
            module(&format!("export const a = action(async (t, p, list) => {{ {fine} }});"));
        let (_, diagnostics) = output(&source, &options());
        assert!(diagnostics.iter().all(|d| d.code != Code::ActionNestedWrite), "{fine}");
    }
    let outside = module("p.then((v) => { t.v = v; });\nexport const a = action(() => {});");
    let (_, diagnostics) = output(&outside, &options());
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}
