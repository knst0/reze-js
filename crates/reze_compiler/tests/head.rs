use reze_compiler::{Code, CompileTarget, Diagnostic, Options, compile};

fn options(target: CompileTarget) -> Options {
    Options { target, module_id: Some("test.tsx".into()), ..Options::default() }
}

fn code_for(source: &str, target: CompileTarget) -> String {
    compile(source, "test.tsx", &options(target)).expect("compiles").expect("rewrites").code
}

fn errors(source: &str) -> Vec<Diagnostic> {
    compile(source, "test.tsx", &Options::default()).err().expect("fails")
}

fn has(errors: &[Diagnostic], code: Code) -> bool {
    errors.iter().any(|d| d.code == code)
}

fn details(errors: &[Diagnostic]) -> Vec<String> {
    errors
        .iter()
        .filter(|d| d.code == Code::HeadAttr)
        .map(|d| d.data.get("detail").expect("detail").clone())
        .collect()
}

#[test]
fn head_emits_tagged_builders_per_target() {
    let source = "import { Head } from \"reze-js\";\n\nexport const view = <Head><script src=\"/sdk.js\" /><meta name=\"theme-color\" content=\"black\" /></Head>;\n";
    let client = code_for(source, CompileTarget::Client);
    assert!(client.contains("\"script:/sdk.js\""), "{client}");
    assert!(client.contains("\"meta:name:theme-color\""), "{client}");
    let island = code_for(source, CompileTarget::Island);
    assert!(island.contains("\"script:/sdk.js\""), "{island}");
    let html = code_for(source, CompileTarget::Html);
    assert!(html.contains("hHead"), "{html}");
    assert!(html.contains("\"script:/sdk.js\""), "{html}");
    assert!(html.contains("\"meta:name:theme-color\""), "{html}");
}

#[test]
fn head_without_src_emits_an_unkeyed_builder() {
    let source = "import { Head } from \"reze-js\";\n\nconst data = { kind: \"Site\" };\n\nexport const view = <Head><script type=\"application/ld+json\">{JSON.stringify(data)}</script></Head>;\n";
    let client = code_for(source, CompileTarget::Client);
    assert!(client.contains("void 0"), "{client}");
}

#[test]
fn head_accepts_literal_keys_in_braces_and_templates() {
    let source = "import { Head } from \"reze-js\";\n\nexport const view = <Head><script src={\"/a.js\"} /><link rel=\"preconnect\" href={`/cdn`} /></Head>;\n";
    let client = code_for(source, CompileTarget::Client);
    assert!(client.contains("\"script:/a.js\""), "{client}");
    assert!(client.contains("\"link:preconnect\\n/cdn\""), "{client}");
}

#[test]
fn head_accepts_dynamic_content() {
    let source = "import { Head, signal } from \"reze-js\";\n\nlet title = signal(\"a\");\n\nexport const view = <Head><meta property=\"og:title\" content={title} /></Head>;\n";
    let client = code_for(source, CompileTarget::Client);
    assert!(client.contains("\"meta:property:og:title\""), "{client}");
}

#[test]
fn head_used_as_a_value_is_an_error() {
    let errors = errors("import { Head } from \"reze-js\";\n\nexport const Alias = Head;\n");
    assert!(has(&errors, Code::ControlFlowAsValue));
}

#[test]
fn head_attributes_are_errors() {
    let errors = errors(
        "import { Head } from \"reze-js\";\n\nexport const view = <Head mount={document.head}><meta name=\"a\" content=\"b\" /></Head>;\n",
    );
    assert!(has(&errors, Code::ControlFlowAttribute));
}

#[test]
fn head_without_children_is_an_error() {
    let errors =
        errors("import { Head } from \"reze-js\";\n\nexport const view = <Head></Head>;\n");
    assert!(has(&errors, Code::ControlFlowChildren));
}

#[test]
fn head_rejects_non_tag_children() {
    for child in [
        "<div />",
        "<Widget />",
        "<Show when={open()}><meta name=\"a\" content=\"b\" /></Show>",
        "<Head><meta name=\"a\" content=\"b\" /></Head>",
        "{open() && <meta name=\"a\" content=\"b\" />}",
        "words",
    ] {
        let source = format!(
            "import {{ Head, Show }} from \"reze-js\";\n\nconst Widget = () => <i />;\n\nexport const view = <Head>{child}</Head>;\n"
        );
        let found = errors(&source);
        assert!(has(&found, Code::ControlFlowChildren), "{child}: {found:?}");
    }
}

#[test]
fn head_rejects_spreads_handlers_and_islands() {
    let source = "import { Head } from \"reze-js\";\n\nexport const view = <Head><meta name=\"a\" content=\"b\" {...attrs} /><script src=\"/a.js\" onLoad={ready} /><link rel=\"preconnect\" href=\"/cdn\" island=\"idle\" /></Head>;\n";
    let found = details(&errors(source));
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found[0].contains("spread"), "{found:?}");
    assert!(found[1].contains("event handler"), "{found:?}");
    assert!(found[2].contains("island"), "{found:?}");
}

#[test]
fn head_rejects_dynamic_identity() {
    let source = "import { Head } from \"reze-js\";\n\nexport const view = <Head><script src={url} /><link rel={rel} href=\"/cdn\" /><meta name={name} content=\"x\" /></Head>;\n";
    let found = details(&errors(source));
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found.iter().all(|detail| detail.contains("string literal")), "{found:?}");
}

#[test]
fn head_rejects_bad_meta_and_link_identity() {
    for (tag, count) in [
        ("<meta content=\"x\" />", 1),
        ("<meta name=\"a\" property=\"b\" content=\"x\" />", 1),
        ("<link rel=\"preconnect\" />", 1),
        ("<link href=\"/cdn\" />", 1),
    ] {
        let source = format!(
            "import {{ Head }} from \"reze-js\";\n\nexport const view = <Head>{tag}</Head>;\n"
        );
        let found = details(&errors(&source));
        assert_eq!(found.len(), count, "{tag}: {found:?}");
    }
}

#[test]
fn head_rejects_executable_inline_scripts() {
    for tag in [
        "<script>alert(1)</script>",
        "<script type=\"module\">import \"/a.js\"</script>",
        "<script type={kind}>data</script>",
    ] {
        let source = format!(
            "import {{ Head }} from \"reze-js\";\n\nexport const view = <Head>{tag}</Head>;\n"
        );
        assert!(has(&errors(&source), Code::HeadAttr), "{tag}");
    }
}

#[test]
fn head_rejects_nested_elements_and_void_children() {
    let source = "import { Head } from \"reze-js\";\n\nexport const view = <Head><script src=\"/a.js\"><b>bold</b></script><meta name=\"a\" content=\"b\">text</meta></Head>;\n";
    let found = details(&errors(source));
    assert_eq!(found.len(), 2, "{found:?}");
}
