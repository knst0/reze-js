use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use reze_compiler::{Options, compile};

fn run(arguments: &str) -> String {
    let source = format!(
        "import {{ $props }} from 'reze-js'; export const props = $props.merge({arguments});"
    );
    let output =
        compile(&source, "t03.ts", &Options::default()).expect("compiles").expect("rewrites");
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &output.code, SourceType::mjs()).parse();
    assert!(parsed.diagnostics.is_empty(), "{}: {:?}", output.code, parsed.diagnostics);
    output.code
}

#[test]
fn empty_sources_produce_valid_literals() {
    for arguments in [
        "",
        "{}",
        "{}, {}",
        "{}, {a: 1}",
        "{a: 1}, {}",
        "{}, { /* empty */ }, {a: 1,}, {}",
        "{a: 1,}, {b: 2,}",
        "{a: 1 // tail\n}, {b: 2}",
    ] {
        let code = run(arguments);
        assert!(!code.contains("mergeProps"), "{code}");
    }
}

#[test]
fn literal_numeric_and_computed_string_keys_dissolve() {
    let code = run("{1: 2, ['label']: 'first'}, {['label']: 'last', [2]: 3}");
    assert!(!code.contains("mergeProps"), "{code}");
    assert!(code.find("first").unwrap() < code.find("last").unwrap(), "{code}");
}

#[test]
fn value_evaluation_order_is_preserved() {
    let code = run("{a: (first(), 1)}, {}, {a: (second(), 2), [3]: (third(), 3)}");
    assert!(!code.contains("mergeProps"), "{code}");
    assert!(code.find("first()").unwrap() < code.find("second()").unwrap(), "{code}");
    assert!(code.find("second()").unwrap() < code.find("third()").unwrap(), "{code}");
}

#[test]
fn uncertain_and_descriptor_sensitive_sources_keep_runtime_merge() {
    for arguments in [
        "{a: 1}, {a: undefined}",
        "{a: value}",
        "{get a() {return 1}}",
        "{set a(value) {}}",
        "{a() {return 1}}",
        "{...source}",
        "{[key]: 1}",
        "{['__proto__']: 1}",
        "{__proto__: null}",
        "{'__proto__': null}",
    ] {
        let code = run(arguments);
        assert!(code.contains("mergeProps"), "{code}");
    }
}

#[test]
fn new_target_values_keep_runtime_merge() {
    for key in ["1", "[1]", "['label']"] {
        for value in ["new.target", "(0, new.target)", "true ? new.target : 1"] {
            let source = format!(
                "import {{ $props }} from 'reze-js'; function f() {{ return $props.merge({{{key}: 7}}, {{{key}: {value}}}); }} f(); new f();"
            );
            let output = compile(&source, "t03.ts", &Options::default()).unwrap().unwrap();
            assert!(output.code.contains("mergeProps"), "{}", output.code);
            assert!(output.code.contains("new.target"), "{}", output.code);
        }
    }
}

#[test]
#[ignore = "requires Node.js 24 via REZE_NODE"]
fn new_target_runtime_preserves_ordinary_and_constructor_calls() {
    let runtime = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/dom/src/props.ts")
        .canonicalize()
        .unwrap();
    for key in ["1", "[1]", "['label']"] {
        let access = if key == "['label']" { "'label'" } else { "1" };
        let source = format!(
            "import {{ $props }} from 'reze-js'; function f() {{ this.value = $props.merge({{{key}: 7}}, {{{key}: new.target}})[{access}]; }} const ordinary = {{}}; f.call(ordinary); if (ordinary.value !== 7) throw Error('ordinary call lost 7'); if (new f().value !== f) throw Error('constructor target lost');"
        );
        let output = compile(&source, "t03.ts", &Options::default()).unwrap().unwrap();
        let code = output
            .code
            .replace("from 'reze-js'", &format!("from {:?}", runtime.to_str().unwrap()))
            .replace("from \"reze-js\"", &format!("from {:?}", runtime.to_str().unwrap()));
        let result = std::process::Command::new(std::env::var("REZE_NODE").expect("REZE_NODE"))
            .args(["--input-type=module", "--eval", &code])
            .output()
            .unwrap();
        assert!(result.status.success(), "{}\n{}", code, String::from_utf8_lossy(&result.stderr));
    }
}
