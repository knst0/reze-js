use reze_compiler::{Code, Diagnostic, Options, Output, compile};

fn output_for(source: &str) -> Output {
    compile(source, "test.tsx", &Options::default()).expect("compiles").expect("rewrites")
}

fn errors(source: &str) -> Vec<Diagnostic> {
    compile(source, "test.tsx", &Options::default()).err().expect("fails")
}

fn warnings(source: &str) -> Vec<Diagnostic> {
    output_for(source).diagnostics
}

#[test]
fn media_island_without_a_query_is_an_error() {
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
}
