mod common;

use common::{checked, compiled, diagnostics, only, without_source_map};
use reze_compiler::{Code, Severity};

fn module(body: &str) -> String {
    format!("import {{ action }} from \"reze-js\";\n{body}")
}

#[test]
fn action_arguments_and_values_are_refused() {
    let options = without_source_map();
    for argument in ["save", "make()", "", "...handlers"] {
        let source = module(&format!("export const a = action({argument});"));
        let d = only(&source, &options, Code::ActionArgument);
        assert_eq!(d.severity, Severity::Error);
    }
    let d = only(&module("export const a = action(save);"), &options, Code::ActionArgument);
    assert_eq!((d.start.line, d.start.column, d.end.column), (2, 24, 28));

    only(&module("export const make = action;"), &options, Code::ActionNotCalled);
    only(&module("register(action);"), &options, Code::ActionNotCalled);
    only(
        "import * as R from \"reze-js\";\nexport const make = R.action;",
        &options,
        Code::ActionNotCalled,
    );
}

#[test]
fn suspensions_the_compiler_cannot_resume_are_refused_but_nested_ones_are_not() {
    let options = without_source_map();
    for (body, construct) in [
        ("async (s) => { for await (const x of s) use(x); }", "for await"),
        ("async () => { await using r = open(); }", "await using"),
        ("function* () { yield 1; }", "generator"),
        ("async function* () {}", "generator"),
    ] {
        let d = only(
            &module(&format!("export const a = action({body});")),
            &options,
            Code::ActionUnsupported,
        );
        assert_eq!(d.data["construct"], construct, "{body}");
    }
    let nested = module(
        "export const a = action(async (s) => { await drain(async () => { for await (const x of s) use(x); }); });",
    );
    compiled(&nested, &options);
}

#[test]
fn a_member_write_in_a_function_that_runs_later_warns_and_one_in_a_sync_callback_does_not() {
    let options = without_source_map();
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
        let d = only(&source, &options, Code::ActionNestedWrite);
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
        let (_, diagnostics) = checked(&source, &options);
        assert!(diagnostics.iter().all(|d| d.code != Code::ActionNestedWrite), "{fine}");
    }
    let outside = module("p.then((v) => { t.v = v; });\nexport const a = action(() => {});");
    assert!(diagnostics(&outside, &options).is_empty(), "{outside}");
}
