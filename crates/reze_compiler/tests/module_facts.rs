use reze_compiler::{
    ExportFacts, FactKind, ModuleFacts, Options, Reads, analyze, compile, compile_with_facts,
};
use std::collections::HashMap;

const LIB: &str = "import { signal, action } from \"reze-js\";\nlet n = signal(0);\nexport function read() { return n; }\nexport function label() { return 'x'; }\nexport function plain(a) { return a + 1; }\nexport function bumpCount() { n++; }\nexport const bump = action(() => { n++; });\nexport const fixed = 3;\nexport { read as reading } from './other';";

const APP: &str = "import { signal, computed } from \"reze-js\";\nimport { read, label } from \"./lib\";\nlet m = signal(1);\nexport const view = <p>{read()}{label()}{m}</p>;\nconst d = computed(read() * 2);\nexport function Button() { return <b onClick={() => { m++; }}>{d}</b>; }";

fn options_with(facts: HashMap<String, ModuleFacts>) -> Options {
    Options { source_map: false, facts, ..Options::default() }
}

fn code(source: &str, options: &Options) -> String {
    compile(source, "app.tsx", options).expect("compiles").expect("rewritten").code
}

#[test]
fn exports_describe_functions_actions_values_and_reexports() {
    let exports = analyze(LIB, "lib.tsx").expect("analyzes").exports;
    assert_eq!(
        exports["read"],
        ExportFacts::Function { reads: Reads::Fixed, returns: Some(FactKind::Number) }
    );
    assert_eq!(
        exports["label"],
        ExportFacts::Function { reads: Reads::None, returns: Some(FactKind::String) }
    );
    assert_eq!(exports["plain"], ExportFacts::Function { reads: Reads::None, returns: None });
    assert_eq!(
        exports["bumpCount"],
        ExportFacts::Function { reads: Reads::Dynamic, returns: None }
    );
    assert_eq!(exports["bump"], ExportFacts::Action);
    assert_eq!(exports["fixed"], ExportFacts::Value);
    assert_eq!(
        exports["reading"],
        ExportFacts::Reexport { from: "./other".to_string(), name: "read".to_string() }
    );
}

#[test]
fn compile_with_facts_reports_analyzed_facts_and_a_module_with_an_error_has_none() {
    let compiled = compile_with_facts(LIB, "lib.tsx", &Options::default()).expect("compiles");
    assert_eq!(compiled.facts, analyze(LIB, "lib.tsx").expect("analyzes"));

    assert!(
        analyze(
            "import { signal } from \"reze-js\";\nlet n = signal(0);\nexport default n;",
            "bad.tsx"
        )
        .is_err()
    );
}

#[test]
fn an_imported_return_kind_joins_the_text_update_it_feeds() {
    let without = code(APP, &Options::default());
    let with = code(
        APP,
        &options_with(HashMap::from([("./lib".to_string(), analyze(LIB, "lib.tsx").unwrap())])),
    );
    assert!(without.contains("_$insert(_el$, read"), "{without}");
    assert!(with.contains("read() + label() + m()"), "{with}");
}

#[test]
fn facts_of_another_compiler_or_format_version_are_ignored() {
    let mut other_compiler = analyze(LIB, "lib.tsx").unwrap();
    other_compiler.compiler = "0.0.0".to_string();
    let mut other_format = analyze(LIB, "lib.tsx").unwrap();
    other_format.v = 0;
    for stale in [other_compiler, other_format] {
        let with_stale = code(APP, &options_with(HashMap::from([("./lib".to_string(), stale)])));
        assert_eq!(with_stale, code(APP, &Options::default()));
    }
}

const COUNTER: &str = "import { signal } from \"reze-js\";\nexport let count = signal(0);\nexport const bump = () => (count += 1);\nexport const doubled = () => count * 2;";

fn counter_facts() -> HashMap<String, ModuleFacts> {
    HashMap::from([("./counter".to_string(), analyze(COUNTER, "counter.tsx").expect("analyzes"))])
}

#[test]
fn imported_signal_reads_lower_to_getter_calls_and_dynamic_readers_are_marked() {
    let app = "import { count } from \"./counter\";\nexport const View = () => <p>{count + 1}</p>;";
    assert!(code(app, &options_with(counter_facts())).contains("count() + 1"));

    let app = "import * as k from \"./counter\";\nexport const View = () => <p>{k.count}</p>;";
    assert!(code(app, &options_with(counter_facts())).contains("k.count()"));

    let reader = "import { count } from \"./counter\";\nexport const twice = () => count * 2;";
    let exports = analyze(reader, "reader.tsx").expect("analyzes").exports;
    let ExportFacts::Function { reads, .. } = &exports["twice"] else {
        panic!("twice is a function: {:?}", exports["twice"]);
    };
    assert_eq!(*reads, Reads::Dynamic);
}
