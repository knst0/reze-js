use reze_compiler::{Options, Output, compile};

fn run(source: &str) -> Output {
    compile(source, "t06.tsx", &Options::default()).expect("compiles").expect("rewrites")
}

#[test]
fn repeated_long_literals_share_only_the_string() {
    let source = "const a = () => <p>abcdefghijklmnopqrstuvwxyz0123456789</p>;\nconst b = () => <p>abcdefghijklmnopqrstuvwxyz0123456789</p>;";
    let out = run(source);
    assert_eq!(out.code.matches("abcdefghijklmnopqrstuvwxyz0123456789").count(), 1, "{}", out.code);
    assert_eq!(out.code.matches("_$template(_$html)").count(), 2, "{}", out.code);
    assert!(out.code.contains("const _$html = "), "{}", out.code);
}

#[test]
fn hoisted_functions_keep_cycle_safe_inline_literals() {
    for declaration in [
        "function A()",
        "export function A()",
        "export default function A()",
        "export default function()",
    ] {
        let source = format!(
            "import 'reze-js'; import './b.mjs'; {declaration} {{ return <p>abcdefghijklmnopqrstuvwxyz0123456789</p>; }} const b = () => <p>abcdefghijklmnopqrstuvwxyz0123456789</p>;"
        );
        let out = run(&source);
        assert!(!out.code.contains("const _$html"), "{}", out.code);
        assert_eq!(
            out.code.matches("abcdefghijklmnopqrstuvwxyz0123456789").count(),
            2,
            "{}",
            out.code
        );
    }
}

#[test]
fn exported_const_arrows_still_share_literals() {
    let out = run(
        "export const A = () => <p>abcdefghijklmnopqrstuvwxyz0123456789</p>; export const B = () => <p>abcdefghijklmnopqrstuvwxyz0123456789</p>;",
    );
    assert!(out.code.contains("const _$html = "), "{}", out.code);
    assert_eq!(out.code.matches("_$template(_$html)").count(), 2);
}

#[test]
fn single_and_short_templates_keep_inline_literals() {
    for source in [
        "const a = <p>abcdefghijklmnopqrstuvwxyz0123456789</p>;",
        "const a = <p/>; const b = <p/>;",
    ] {
        assert!(!run(source).code.contains("const _$html"));
    }
}

#[test]
fn directives_imports_and_collision_names_are_preserved() {
    let out = run(
        "\"use client\";\nimport x from 'x';\nconst _$html = x; const a = <p>abcdefghijklmnopqrstuvwxyz0123456789</p>; const b = <p>abcdefghijklmnopqrstuvwxyz0123456789</p>;",
    );
    assert!(out.code.starts_with("\"use client\";\nimport x from 'x';"), "{}", out.code);
    assert!(out.code.contains("const _$html2 = "), "{}", out.code);
    assert_eq!(out.code.matches("_$template(_$html2)").count(), 2);
}

#[test]
fn unicode_marks_follow_replacements_and_inserted_declaration() {
    let source = "const a = <p>😀abcdefghijklmnopqrstuvwxyz0123456789</p>; const tail = '😀';\nconst b = <p>😀abcdefghijklmnopqrstuvwxyz0123456789</p>; const end = 2;";
    let out = run(source);
    assert!(out.code.contains("const _$html = "));
    let map = oxc_sourcemap::SourceMap::from_json_string(out.map.as_deref().unwrap()).unwrap();
    for suffix in ["; const tail", "; const end"] {
        let source_offset = source.find(suffix).unwrap();
        let generated_offset = out.code.find(suffix).unwrap();
        let position = |text: &str, offset: usize| {
            let prefix = &text[..offset];
            (
                prefix.bytes().filter(|b| *b == b'\n').count() as u32,
                prefix.rsplit('\n').next().unwrap().encode_utf16().count() as u32,
            )
        };
        let dst = position(&out.code, generated_offset);
        let src = position(source, source_offset);
        let token = map
            .get_tokens()
            .find(|t| (t.get_dst_line(), t.get_dst_col()) == dst)
            .expect("suffix token");
        assert_eq!((token.get_src_line(), token.get_src_col()), src);
    }
}

#[test]
fn only_strict_byte_savings_extract_a_constant() {
    for (length, extracts) in [(24, false), (25, true)] {
        let text = "x".repeat(length);
        let source = format!("const a = <p>{text}</p>; const b = <p>{text}</p>;");
        let out = run(&source);
        assert_eq!(out.code.contains("const _$html = "), extracts, "{}", out.code);
        if extracts {
            let literal = format!("\"<p>{text}\"");
            let inline = out
                .code
                .replace(&format!("\nconst _$html = {literal};"), "")
                .replace("(_$html)", &format!("({literal})"));
            assert_eq!(inline.len() - out.code.len(), 1);
        }
    }
}

#[test]
fn distinct_literals_and_namespaces_keep_their_factories() {
    let source = "const a = <g><path d=\"abcdefghijklmnopqrstuvwxyz0123456789\"/></g>; const b = <g><path d=\"abcdefghijklmnopqrstuvwxyz0123456789\"/></g>; const c = <math><mi>abcdefghijklmnopqrstuvwxyz0123456789</mi></math>; const d = <math><mi>abcdefghijklmnopqrstuvwxyz0123456789</mi></math>; const e = <p>ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789</p>; const f = <p>ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789</p>;";
    let out = run(source);
    assert_eq!(out.code.matches("const _$html").count(), 3, "{}", out.code);
    assert_eq!(out.code.matches("_$templateSVG(_$html)").count(), 2, "{}", out.code);
    assert_eq!(out.code.matches("_$templateMathML(_$html2)").count(), 2, "{}", out.code);
    assert_eq!(out.code.matches("_$template(_$html3)").count(), 2, "{}", out.code);
    assert!(out.code.contains("<svg>"), "{}", out.code);
}

#[test]
fn escaped_literals_and_multiple_groups_relocate_source_marks() {
    let source = "const a = <p title={'a\"b'}>abcdefghijklmnopqrstuvwxyz0123456789</p>; const b = <p title={'a\"b'}>abcdefghijklmnopqrstuvwxyz0123456789</p>; const c = <p>differentabcdefghijklmnopqrstuvwxyz0123456789</p>; const d = <p>differentabcdefghijklmnopqrstuvwxyz0123456789</p>; const end = 1;";
    let out = run(source);
    assert_eq!(out.code.matches("const _$html").count(), 2, "{}", out.code);
    assert_eq!(out.code.matches("_$template(").count(), 4);
    let map = oxc_sourcemap::SourceMap::from_json_string(out.map.as_deref().unwrap()).unwrap();
    let generated_line =
        out.code.lines().position(|line| line.contains("const end = 1")).unwrap() as u32;
    let generated_col =
        out.code.lines().nth(generated_line as usize).unwrap().find("; const end = 1").unwrap()
            as u32;
    let source_col = source.find("; const end = 1").unwrap() as u32;
    let token = map
        .get_tokens()
        .find(|t| t.get_dst_line() == generated_line && t.get_dst_col() == generated_col)
        .unwrap();
    assert_eq!((token.get_src_line(), token.get_src_col()), (0, source_col));
}
