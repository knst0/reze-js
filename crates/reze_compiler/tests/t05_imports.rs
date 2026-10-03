use reze_compiler::{Options, compile};

fn run(source: &str) -> String {
    compile(source, "t05.tsx", &Options::default()).expect("compiles").expect("rewrites").code
}

#[test]
fn removes_sole_signal_import_after_every_factory_call_folds() {
    for module in ["reze-js", "@rezejs/signals", "@rezejs/dom"] {
        let code = run(&format!(
            "import {{ signal as make }} from '{module}'; const [a] = make(1); const [b] = make(2); console.log(a(), b());"
        ));
        assert!(!code.contains("signal as make"), "{code}");
        assert!(!code.contains("make("), "{code}");
        println!("T05 {module}: {} bytes; {code}", code.len());
    }
}

#[test]
fn preserves_mixed_escaped_exported_and_type_imports() {
    for suffix in [
        "console.log(make);",
        "export { make };",
        "const [b, set] = make(2); set(3);",
        "type Factory = typeof make;",
        "function escaped() { return eval('make'); }",
    ] {
        let code = run(&format!(
            "import {{ signal as make }} from 'reze-js'; const [a] = make(1); console.log(a()); {suffix}"
        ));
        assert!(code.contains("signal as make"), "{code}");
    }
    let code = run(
        "import { signal as make, type Signal } from 'reze-js'; const [a] = make(1); console.log(a());",
    );
    assert!(code.contains("signal as make, type Signal"), "{code}");
}

#[test]
fn preserves_unknown_module_effects_and_shadowed_factory() {
    let code = run(
        "import { signal } from 'custom-effects'; const [a] = signal(1); const view = <p>{a()}</p>;",
    );
    assert!(code.contains("from 'custom-effects'"), "{code}");
    let code = run(
        "import { signal as make } from 'reze-js'; const [a] = make(1); function nested(make) { return make(2); } console.log(a());",
    );
    assert!(!code.contains("signal as make"), "{code}");
    assert!(code.contains("return make(2)"), "{code}");
    let code = run(
        "import 'custom-effects'; import { signal as make } from 'reze-js'; const [a] = make(1); console.log(a());",
    );
    assert!(code.contains("import 'custom-effects'"), "{code}");
}

#[test]
fn import_removal_preserves_source_marks_after_the_folded_declaration() {
    let source = "import { signal as make } from 'reze-js'; const [a] = make(1); console.log(a()); const end = 2;";
    let out = compile(source, "t05.tsx", &Options::default()).unwrap().unwrap();
    let map = oxc_sourcemap::SourceMap::from_json_string(out.map.as_deref().unwrap()).unwrap();
    let generated_col = out.code.find("); const end = 2").unwrap() as u32;
    let source_col = source.find("); const end = 2").unwrap() as u32;
    let token = map
        .get_tokens()
        .find(|t| t.get_dst_line() == 0 && t.get_dst_col() == generated_col)
        .unwrap();
    assert_eq!((token.get_src_line(), token.get_src_col()), (0, source_col));
}
