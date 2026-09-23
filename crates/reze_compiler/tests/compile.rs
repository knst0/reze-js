use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use reze_compiler::{Options, compile};

fn run(source: &str) -> String {
    compile(source, "test.tsx", &Options::default()).unwrap().expect("has JSX").code
}

/// The HTML of every `template()` in `code`, in declaration order.
fn templates(code: &str) -> Vec<String> {
    code.split("_$template(\"")
        .skip(1)
        .map(|rest| {
            let mut html = String::new();
            let mut chars = rest.chars();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => match chars.next() {
                        Some('n') => html.push('\n'),
                        Some(c) => html.push(c),
                        None => break,
                    },
                    c => html.push(c),
                }
            }
            html
        })
        .collect()
}

/// Output must be valid TSX with no duplicate declarations.
fn assert_valid(code: &str) {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, SourceType::tsx()).parse();
    assert!(parsed.diagnostics.is_empty(), "{:?}\n{code}", parsed.diagnostics);
    let semantic = SemanticBuilder::new().with_check_syntax_error(true).build(&parsed.program);
    assert!(semantic.diagnostics.is_empty(), "{:?}\n{code}", semantic.diagnostics);
}

#[test]
fn file_without_jsx_is_left_alone() {
    assert!(compile("const a = 1 < 2;", "a.ts", &Options::default()).unwrap().is_none());
}

#[test]
fn syntax_errors_report_their_position() {
    let errors = compile("const a = 1;\nconst b = <div>;", "a.tsx", &Options::default())
        .err()
        .expect("invalid JSX");
    assert_eq!(errors[0].line, 2);
}

#[test]
fn a_marker_separates_an_insertion_from_texts_the_parser_would_merge() {
    let code = run("const a = <p>hi {name()}!</p>;\nconst b = <p>{a()}:{b()}<i/>{c()}</p>;");
    // Between two texts the parser would merge "hi " and "!", losing the insertion point.
    // After an element or at the edges, the neighbour node itself is a stable anchor.
    assert_eq!(templates(&code), ["<p>hi <!>!</p>", "<p>:<i></i></p>"]);
    assert_valid(&code);
}

#[test]
fn text_follows_jsx_whitespace_and_entity_rules() {
    let code = run(
        "const a = (\n  <p title=\"x &amp; &quot;y&quot;\">\n    a &amp; b\n    c&nbsp;&#x41;&#66; {\"<&>\"}\n  </p>\n);",
    );
    assert_eq!(
        templates(&code),
        ["<p title=\"x &amp; &quot;y&quot;\">a &amp; b c\u{a0}AB &lt;&amp;></p>"]
    );
}

#[test]
fn component_text_children_are_decoded_strings() {
    let code = run("const a = <Button label=\"a &lt; b\">Save &amp; exit</Button>;");
    assert!(code.contains(r#"label: "a < b""#), "{code}");
    assert!(code.contains(r#"children: "Save & exit""#), "{code}");
}

#[test]
fn an_svg_child_template_is_parsed_inside_svg() {
    let code = run("const a = <path d=\"M0\" />;\nconst b = <svg><path /></svg>;");
    assert!(
        code.contains(r#"_$template("<svg><path d=\"M0\"></path></svg>", false, true)"#),
        "{code}"
    );
    assert!(code.contains(r#"_$template("<svg><path></path></svg>")"#), "{code}");
}

#[test]
fn generated_names_avoid_names_in_the_source() {
    let code = run("const _tmpl$ = 1, _el$ = 2, _$insert = 3, _p$ = 4;\n\
         export const a = <p class={c()} title={t()}>{_tmpl$}{_el$}{_$insert}{_p$}<b/></p>;");
    assert_valid(&code);
}

#[test]
fn runtime_imports_and_templates_follow_the_leading_imports() {
    let code = run("import { a } from \"a\";\nimport b from \"b\";\nconst x = <i/>;");
    let runtime = code.find("from \"reze-js\"").unwrap();
    assert!(code.find("from \"b\"").unwrap() < runtime);
    assert!(runtime < code.find("const x").unwrap());
    assert_valid(&code);
}

#[test]
fn delegated_events_are_registered_once_per_module() {
    let code = run("const a = <><b onClick={f} /><i onClick={() => g()} onInput={h} /></>;");
    assert!(code.trim_end().ends_with(r#"_$delegateEvents(["click", "input"]);"#), "{code}");
    assert_valid(&code);
}

#[test]
fn source_map_points_generated_code_back_at_its_jsx() {
    let source = "const n = 1;\nconst a = <p>{n}</p>;\n";
    let out = compile(source, "test.tsx", &Options::default()).unwrap().unwrap();
    let map = oxc_sourcemap::SourceMap::from_json_string(out.map.as_deref().unwrap()).unwrap();
    let line = out.code.lines().position(|l| l.starts_with("const a")).unwrap() as u32;
    let token = map
        .get_tokens()
        .find(|t| t.get_dst_line() == line && t.get_dst_col() == 10)
        .expect("token at the compiled JSX");
    assert_eq!((token.get_src_line(), token.get_src_col()), (1, 10));
}
