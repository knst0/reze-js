use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use reze_compiler::{Options, compile};

fn run(source: &str) -> String {
    compile(source, "test.tsx", &Options::default()).unwrap().expect("has JSX").code
}

/// The HTML of every `template*()` factory call in `code`, in declaration order.
fn templates(code: &str) -> Vec<String> {
    code.split("_$template")
        .skip(1)
        .filter_map(|rest| {
            let rest = rest.strip_prefix("SVG").or(rest.strip_prefix("MathML")).unwrap_or(rest);
            let rest = rest.strip_prefix("(\"")?;
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
            Some(html)
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
        code.contains(r#"_$templateSVG("<svg><path d=\"M0\"></path></svg>")"#),
        "{code}"
    );
    // An `<svg>` root parses natively: no wrapper, plain factory.
    assert!(code.contains(r#"_$template("<svg><path></path></svg>")"#), "{code}");
}

#[test]
fn suspicious_patterns_produce_warnings_not_errors() {
    // `key` on a plain element renders as a useless attribute (D03).
    let out = compile("const a = <div key=\"x\" />;", "a.tsx", &Options::default())
        .unwrap()
        .unwrap();
    assert_eq!(out.warnings.len(), 1);
    assert!(out.warnings[0].message.contains("`key`"), "{}", out.warnings[0].message);
    assert!(out.code.contains("key="), "{}", out.code);
    // `children` attribute alongside nested children: the attribute loses (D05).
    let out = compile("const a = <div children={x()}><b /></div>;", "a.tsx", &Options::default())
        .unwrap()
        .unwrap();
    assert_eq!(out.warnings.len(), 1);
    assert!(out.warnings[0].message.contains("`children`"), "{}", out.warnings[0].message);
    // `children` attribute alone keeps working without warnings.
    let out = compile("const a = <div children={x()} />;", "a.tsx", &Options::default())
        .unwrap()
        .unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    // Inline array `each` rebuilds rows per evaluation (D04).
    let out = compile(
        "const a = <For each={[1, 2]}>{(x) => x}</For>;",
        "a.tsx",
        &Options::default(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(out.warnings.len(), 1);
    assert!(out.warnings[0].message.contains("inline array"), "{}", out.warnings[0].message);
    // A stable `each` source stays quiet.
    let out = compile("const a = <For each={items}>{(x) => x}</For>;", "a.tsx", &Options::default())
        .unwrap()
        .unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
fn near_miss_attributes_suggest_the_real_name() {
    for (typo, suggestion) in [("clas", "class"), ("stlye", "style"), ("viewbox", "viewBox")] {
        let out = compile(&format!("const a = <div {typo}=\"x\" />;"), "a.tsx", &Options::default())
            .unwrap()
            .unwrap();
        assert_eq!(out.warnings.len(), 1, "{typo}");
        assert!(
            out.warnings[0].message.contains(&format!("did you mean `{suggestion}`")),
            "{}",
            out.warnings[0].message
        );
        // The attribute still renders untouched.
        assert!(out.code.contains(typo), "{}", out.code);
    }
    // Exact names, custom `data-*`, and hyphenated names stay quiet.
    for quiet in ["class", "data-count", "stroke-width", "myattr"] {
        let out = compile(&format!("const a = <div {quiet}=\"x\" />;"), "a.tsx", &Options::default())
            .unwrap()
            .unwrap();
        assert!(out.warnings.is_empty(), "{quiet}: {:?}", out.warnings);
    }
}

#[test]
fn generated_names_avoid_names_in_the_source() {
    let code = run("const _tmpl$ = 1, _el$ = 2, _$insert = 3, _p$ = 4;\n\
         export const a = <p class={c()} title={t()}>{_tmpl$}{_el$}{_$insert}{_p$}<b/></p>;");
    assert_valid(&code);
}

#[test]
fn static_string_concatenation_folds_but_number_addition_does_not() {
    let code = run(r#"const a = <div title={"a" + "b" + `c`}>{"x" + 1}</div>;"#);
    assert_eq!(templates(&code), vec![r#"<div title="abc">x1</div>"#]);
    // `1 + 2` is 3, not "12": stays dynamic. So does anything with a variable.
    let code = run("const a = <div title={1 + 2}>{\"x\" + y}</div>;");
    assert!(!code.contains("title=\""), "{code}");
    assert!(code.contains("1 + 2"), "{code}");
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
    // First-use order is normalized: reversed source still emits the sorted trailer (C12).
    let code = run("const a = <><b onInput={f} /><i onClick={g} /></>;");
    assert!(code.trim_end().ends_with(r#"_$delegateEvents(["click", "input"]);"#), "{code}");
    assert_valid(&code);
}

#[test]
fn a_math_root_uses_the_mathml_factory() {
    // `<math>` at the template root parses under the MathML namespace (C23/B02);
    // nested `<math>` needs no flag: the HTML parser namespaces it itself.
    let code = run("const a = <math><mi>{x}</mi></math>;");
    assert!(code.contains(r#"_$templateMathML("<math><mi></mi></math>")"#), "{code}");
    assert_eq!(templates(&code), vec!["<math><mi></mi></math>"]);
    let code = run("const a = <div><math><mi>x</mi></math></div>;");
    assert!(!code.contains("templateMathML"), "{code}");
    assert_valid(&code);
}

#[test]
fn all_literal_style_objects_fold_into_the_template() {
    // No `style` runtime import: the object becomes a static attribute (C09).
    let code = run(r#"const a = <div style={{color: "red", "font-size": "12px"}} />;"#);
    assert_eq!(templates(&code), vec![r#"<div style="color:red;font-size:12px"></div>"#]);
    assert!(!code.contains("_$style"), "{code}");
    // Anything dynamic keeps the runtime call.
    let code = run("const a = <div style={{color: c}} />;");
    assert!(code.contains("_$style"), "{code}");
    assert_valid(&code);
}

#[test]
fn all_literal_class_lists_fold_into_the_template() {
    // Truthy keys become tokens, falsy keys drop, no `classList` import (C10).
    let code = run(r#"const a = <div classList={{active: true, hidden: false, "a b": 1}} />;"#);
    assert_eq!(templates(&code), vec![r#"<div class="active a b"></div>"#]);
    assert!(!code.contains("_$classList"), "{code}");
    let code = run("const a = <div classList={{active: on}} />;");
    assert!(code.contains("_$classList"), "{code}");
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

#[test]
fn bare_call_children_skip_the_getter_closure_but_methods_keep_it() {
    // `f()` passes `f` itself so the runtime subscribes to the getter (C19).
    let code = run("const a = <div>{f()}</div>;");
    assert!(code.contains(", f)"), "{code}");
    assert!(!code.contains("() => f()"), "{code}");
    // `obj.m()` must keep the arrow: calling `m` bare would lose its receiver.
    let code = run("const a = <div>{obj.m()}</div>;");
    assert!(code.contains("() => obj.m()"), "{code}");
    assert_valid(&code);
}

#[test]
fn search_is_a_void_element() {
    let code = run("const a = <search />;");
    assert_eq!(templates(&code), vec!["<search>"]);
    assert_valid(&code);
}

#[test]
fn svg2_hatch_roots_parse_inside_svg() {
    let code = run("const a = <hatch><hatchpath /></hatch>;");
    assert_eq!(templates(&code), vec!["<svg><hatch><hatchpath></hatchpath></hatch></svg>"]);
    assert_valid(&code);
}

#[test]
fn single_await_async_component_becomes_sync_with_tracked_subscription() {
    let code = run(
        "async function User(props) { \
           const user = await fetchUser(props.id); \
           return <div>{user.name}</div>; \
         }",
    );
    assert!(!code.contains("async function"), "{code}");
    assert!(code.contains("getOwner as "), "{code}");
    assert!(code.contains("trackAsync as "), "{code}");
    assert!(code.contains("Promise.resolve(fetchUser(props.id))"), "{code}");
    assert!(code.contains("const user = "), "{code}");
    assert!(code.contains("if ("), "{code}");
    assert_valid(&code);
}

#[test]
fn async_arrow_component_rewrites_the_same_way() {
    let code = run(
        "const User = async (props) => { \
           const user = await fetchUser(props.id); \
           return <div>{user.name}</div>; \
         };",
    );
    assert!(!code.contains("async"), "{code}");
    assert!(code.contains("=>"), "{code}");
    assert!(code.contains("trackAsync as "), "{code}");
    assert_valid(&code);
}

#[test]
fn chained_awaits_become_linked_fetch_effects() {
    let out = compile(
        "async function User(props) { \
           const a = await f(props.id); \
           const b = await g(a); \
           return <div>{b}</div>; \
         }",
        "test.tsx",
        &Options::default(),
    )
    .unwrap()
    .expect("has JSX");
    assert!(!out.code.contains("async function User"), "{}", out.code);
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    assert_eq!(out.code.matches("Promise.resolve(").count(), 2, "{}", out.code);
    assert!(out.code.contains("Promise.resolve(f(props.id))"), "{}", out.code);
    assert!(out.code.contains("Promise.resolve(g(a))"), "{}", out.code);
    assert!(out.code.contains("const a = "), "{}", out.code);
    assert!(out.code.contains("const b = "), "{}", out.code);
    assert_valid(&out.code);
}

#[test]
fn prefix_locals_used_after_await_keep_async_and_warn() {
    let out = compile(
        "async function User(props) { \
           const label = props.label; \
           const user = await fetchUser(props.id); \
           return <div>{label}{user.name}</div>; \
         }",
        "test.tsx",
        &Options::default(),
    )
    .unwrap()
    .expect("has JSX");
    assert!(out.code.contains("async function User"), "{}", out.code);
    assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
    assert_valid(&out.code);
}

#[test]
fn plain_async_helpers_stay_untouched_without_warnings() {
    let out = compile(
        "async function save(id) { await store(id); } \
         const el = <div>hi</div>;",
        "test.tsx",
        &Options::default(),
    )
    .unwrap()
    .expect("has JSX");
    assert!(out.code.contains("async function save"), "{}", out.code);
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    assert_valid(&out.code);
}

#[test]
fn promise_return_type_unwraps_to_the_sync_shape() {
    let code = run(
        "async function User(props): Promise<string> { \
           const user = await fetchUser(props.id); \
           return <div>{user}</div>; \
         }",
    );
    assert!(code.contains("function User(props): string {"), "{code}");
    assert!(!code.contains("Promise<string>"), "{code}");
    assert_valid(&code);
}

#[test]
fn mid_segment_locals_used_later_keep_async_and_warn() {
    let out = compile(
        "async function User(props) { \
           const a = await f(props.id); \
           const label = a.name; \
           const b = await g(a); \
           return <div>{label}{b}</div>; \
         }",
        "test.tsx",
        &Options::default(),
    )
    .unwrap()
    .expect("has JSX");
    assert!(out.code.contains("async function User"), "{}", out.code);
    assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
    assert_valid(&out.code);
}
