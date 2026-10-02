use reze_compiler::{Code, Diagnostic, Options, Output, compile};

fn output_for(source: &str) -> Output {
    compile(source, "test.tsx", &Options::default()).expect("compiles").expect("rewrites")
}

fn run(source: &str) -> String {
    output_for(source).code
}

fn errors(source: &str) -> Vec<Diagnostic> {
    compile(source, "test.tsx", &Options::default()).err().expect("fails")
}

fn warnings(source: &str) -> Vec<Diagnostic> {
    output_for(source).diagnostics
}

#[test]
fn bare_island_is_eager_with_a_direct_loader() {
    let code = run(
        "function Counter(props) {\n  return <p>{props.step}</p>;\n}\nexport const view = <Counter island step={1} />;",
    );
    assert!(code.contains("(\"eager\", () => Counter,"), "{code}");
    assert!(!code.contains("island:"), "{code}");
}

#[test]
fn island_props_evaluate_at_hydration() {
    let code = run(
        "function Counter(props) {\n  return <p>{props.step}</p>;\n}\nexport const view = <Counter island step={1} label=\"x\" />;",
    );
    assert!(code.contains("get step() { return 1; }"), "{code}");
    assert!(code.contains("label: \"x\""), "{code}");
}

#[test]
fn island_children_evaluate_at_hydration() {
    let code = run(
        "function Panel(props) {\n  return <section>{props.children}</section>;\n}\nexport const view = <Panel island>{greet(name)}</Panel>;",
    );
    assert!(code.contains("get children()"), "{code}");
}

#[test]
fn imported_island_splits_into_its_own_chunk() {
    let code = run(
        "import { Counter } from \"./Counter\";\nexport const view = <Counter island=\"visible\" step={1} />;",
    );
    assert!(!code.contains("from \"./Counter\""), "{code}");
    assert!(code.contains("() => import(\"./Counter\").then("), "{code}");
    assert!(code.contains("Counter"), "{code}");
    assert!(code.contains("\"visible\""), "{code}");
}

#[test]
fn island_fallback_and_root_margin_emit() {
    let code = run(
        "function Counter(props) {\n  return <p />;\n}\nexport const view = <Counter island=\"visible\" islandRootMargin=\"400px\" islandFallback={<b>soon</b>} />;",
    );
    assert!(code.contains("{ rootMargin: \"400px\" }"), "{code}");
    assert!(code.contains("() => "), "{code}");
}

#[test]
fn media_island_needs_its_query() {
    let code = run(
        "import { Counter } from \"./Counter\";\nexport const view = <Counter island=\"media\" islandMedia=\"(max-width: 40rem)\" />;",
    );
    assert!(code.contains("{ media: \"(max-width: 40rem)\" }"), "{code}");
    let diagnostics = errors(
        "import { Counter } from \"./Counter\";\nexport const view = <Counter island=\"media\" />;",
    );
    assert!(diagnostics.iter().any(|d| d.code == Code::IslandMediaMissing), "{diagnostics:?}");
}

#[test]
fn unknown_trigger_is_an_error() {
    let diagnostics = errors(
        "import { Counter } from \"./Counter\";\nexport const view = <Counter island=\"sometimes\" />;",
    );
    assert!(diagnostics.iter().any(|d| d.code == Code::IslandTrigger), "{diagnostics:?}");
}

#[test]
fn island_on_a_native_element_is_an_error() {
    let diagnostics = errors("export const view = <section island=\"visible\" />;");
    assert!(diagnostics.iter().any(|d| d.code == Code::IslandOnElement), "{diagnostics:?}");
}

#[test]
fn island_companion_without_island_is_an_error() {
    let diagnostics = errors(
        "import { Counter } from \"./Counter\";\nexport const view = <Counter islandMedia=\"(max-width: 40rem)\" />;",
    );
    assert!(diagnostics.iter().any(|d| d.code == Code::IslandOrphan), "{diagnostics:?}");
}

#[test]
fn shared_island_stays_in_the_main_chunk_with_a_warning() {
    let found = warnings(
        "import { Counter } from \"./Counter\";\nexport const first = <Counter island=\"visible\" />;\nexport const second = <Counter step={1} />;",
    );
    assert!(found.iter().any(|d| d.code == Code::IslandNotSplit), "{found:?}");
    let code = run(
        "import { Counter } from \"./Counter\";\nexport const first = <Counter island=\"visible\" />;\nexport const second = <Counter step={1} />;",
    );
    assert!(code.contains("from \"./Counter\""), "{code}");
    assert!(code.contains("() => Counter"), "{code}");
}

#[test]
fn namespaced_island_splits_on_its_member() {
    let code =
        run("import * as UI from \"./ui\";\nexport const view = <UI.Counter island=\"idle\" />;");
    assert!(!code.contains("from \"./ui\""), "{code}");
    assert!(code.contains("() => import(\"./ui\").then("), "{code}");
    assert!(code.contains(".Counter)"), "{code}");
}
