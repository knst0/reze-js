use reze_compiler::{Options, compile};

fn run(source: &str) -> String {
    compile(source, "t04.tsx", &Options::default()).expect("compiles").expect("rewrites").code
}

#[test]
fn static_template_interpolations_preserve_primitive_coercion() {
    let code =
        run(r#"const view = <p>{`a${1 + 2}:${"x" + 2}:${-0}:${true}:${null}:${undefined}z`}</p>;"#);
    assert!(code.contains("<p>a3:x2:0:true:null:undefinedz"), "{code}");
}

#[test]
fn numeric_addition_is_not_string_concatenation() {
    let code = run(r#"const view = <p>{1 + 2}</p>;"#);
    assert!(code.contains("1 + 2"), "{code}");
    let code = run(r#"const view = <p>{1 + 2 + "x"}:{"x" + 1 + 2}</p>;"#);
    assert!(code.contains("<p>3x:x12"), "{code}");
}

#[test]
fn negative_zero_nan_and_infinities_have_numeric_truthiness() {
    let code = run(
        r#"const view = <p>{-0 ? "wrong" : "zero"}{(0 / 0) ? "wrong" : "nan"}{(1 / 0) ? "infinity" : "wrong"}</p>;"#,
    );
    assert!(code.contains("<p>zeronaninfinity"), "{code}");
}

#[test]
fn shadowed_undefined_does_not_remove_a_conditional_branch() {
    let code =
        run(r#"export function View(undefined) { return <p>{undefined ? "yes" : "no"}</p>; }"#);
    assert!(code.contains("undefined ?"), "{code}");
}

#[test]
fn template_interpolation_retains_unknown_and_shadowed_values() {
    let code = run(
        r#"export function View(undefined, value) { return <p>{`a${undefined}:${value}`}</p>; }"#,
    );
    assert!(code.contains("${undefined}"), "{code}");
    assert!(code.contains("${value}"), "{code}");
}

#[test]
fn global_nonfinite_numbers_fold_truthiness_but_keep_their_text_at_runtime() {
    let code = run(
        r#"const view = <p>{NaN ? "wrong" : "nan"}{-Infinity ? "infinity" : "wrong"}{`${NaN}:${Infinity}:${1.5}:${1e20}`}</p>;"#,
    );
    assert!(!code.contains("wrong"), "{code}");
    assert!(code.contains("${NaN}"), "{code}");
    assert!(code.contains("${Infinity}"), "{code}");
    assert!(code.contains("${1.5}"), "{code}");
}

#[test]
fn shadowed_nan_and_infinity_keep_both_conditional_branches() {
    let code = run(
        r#"export function View(NaN, Infinity) { return <p>{NaN ? "yes" : "no"}{Infinity ? "on" : "off"}</p>; }"#,
    );
    assert!(code.contains("NaN ?"), "{code}");
    assert!(code.contains("Infinity ?"), "{code}");
}

#[test]
fn nested_templates_and_cooked_escapes_fold_to_rendered_text() {
    let code = run(r#"const view = <p>{`a\u0062${`c${2 - 1}`}\x64`}</p>;"#);
    assert!(code.contains("<p>abc1d"), "{code}");
}

#[test]
fn folded_signal_numeric_zero_is_false_but_string_zero_is_true() {
    let code = run(
        r#"import { signal } from "reze-js"; const [zero] = signal(0); const [text] = signal("0"); const view = <p>{zero() ? "wrong" : "zero"}{text() ? "text" : "wrong"}</p>;"#,
    );
    assert!(code.contains("<p>zerotext"), "{code}");
    assert!(!code.contains("wrong"), "{code}");
}

#[test]
fn falsy_numeric_logical_and_preserves_the_left_rendered_value() {
    for expression in ["1 - 1", "NaN", "0 / 0"] {
        let code = run(&format!("const view = <p>{{({expression}) && <b/>}}</p>;"));
        assert!(code.contains(expression), "{code}");
        assert!(!code.contains("<b"), "{code}");
    }
    let code = run("const view = <p>{-0 && <b/>}</p>;");
    assert!(code.contains("<p>0"), "{code}");
    let code = run(
        r#"import { signal } from "reze-js"; const [zero] = signal(0); const view = <p>{zero() && <b/>}</p>;"#,
    );
    assert!(code.contains("<p>0"), "{code}");
}

#[test]
fn direct_shadowed_undefined_child_is_retained() {
    let code = run("export function View(undefined) { return <p>{undefined}</p>; }");
    assert!(code.contains("_$insert("), "{code}");
    assert!(code.contains("undefined"), "{code}");
    let code = run("const view = <p>{undefined}{false && <b/>}{null && <b/>}</p>;");
    assert!(!code.contains("<b"), "{code}");
    assert!(!code.contains("_$insert("), "{code}");
}
