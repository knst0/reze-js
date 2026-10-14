mod common;

use common::{errors, without_source_map};
use reze_compiler::{Code, CompileTarget, Diagnostic, Options, compile};

fn code_for(source: &str, target: CompileTarget) -> String {
    let options = Options { target, module_id: Some("test.tsx".into()), ..Options::default() };
    compile(source, "test.tsx", &options).expect("compiles").expect("rewrites").code
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
fn head_builders_are_tagged_per_target_and_unkeyed_without_src() {
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

    let source = "import { Head } from \"reze-js\";\n\nconst data = { kind: \"Site\" };\n\nexport const view = <Head><script type=\"application/ld+json\">{JSON.stringify(data)}</script></Head>;\n";
    let client = code_for(source, CompileTarget::Client);
    assert!(client.contains("void 0"), "{client}");
}

#[test]
fn literal_and_dynamic_identities_are_accepted() {
    let source = "import { Head } from \"reze-js\";\n\nexport const view = <Head><script src={\"/a.js\"} /><link rel=\"preconnect\" href={`/cdn`} /></Head>;\n";
    let client = code_for(source, CompileTarget::Client);
    assert!(client.contains("\"script:/a.js\""), "{client}");
    assert!(client.contains("\"link:preconnect\\n/cdn\""), "{client}");

    let source = "import { Head, signal } from \"reze-js\";\n\nlet title = signal(\"a\");\n\nexport const view = <Head><meta property=\"og:title\" content={title} /></Head>;\n";
    let client = code_for(source, CompileTarget::Client);
    assert!(client.contains("\"meta:property:og:title\""), "{client}");
}

#[test]
fn head_children_that_are_misused_are_refused() {
    let options = without_source_map();
    let found =
        errors("import { Head } from \"reze-js\";\n\nexport const Alias = Head;\n", &options);
    assert!(has(&found, Code::ControlFlowAsValue));

    let found = errors(
        "import { Head } from \"reze-js\";\n\nexport const view = <Head mount={document.head}><meta name=\"a\" content=\"b\" /></Head>;\n",
        &options,
    );
    assert!(has(&found, Code::ControlFlowAttribute));

    let found = errors(
        "import { Head } from \"reze-js\";\n\nexport const view = <Head></Head>;\n",
        &options,
    );
    assert!(has(&found, Code::ControlFlowChildren));

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
        let found = errors(&source, &options);
        assert!(has(&found, Code::ControlFlowChildren), "{child}: {found:?}");
    }
    let source = "import { Head } from \"reze-js\";\n\nexport const view = <Head><script src=\"/a.js\"><b>bold</b></script><meta name=\"a\" content=\"b\">text</meta></Head>;\n";
    let found = details(&errors(source, &options));
    assert_eq!(found.len(), 2, "{found:?}");
}

#[test]
fn head_spreads_handlers_islands_identities_and_inline_scripts_are_refused() {
    let options = without_source_map();
    let source = "import { Head } from \"reze-js\";\n\nexport const view = <Head><meta name=\"a\" content=\"b\" {...attrs} /><script src=\"/a.js\" onLoad={ready} /><link rel=\"preconnect\" href=\"/cdn\" island=\"idle\" /></Head>;\n";
    let found = details(&errors(source, &options));
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found[0].contains("spread"), "{found:?}");
    assert!(found[1].contains("event handler"), "{found:?}");
    assert!(found[2].contains("island"), "{found:?}");

    let source = "import { Head } from \"reze-js\";\n\nexport const view = <Head><script src={url} /><link rel={rel} href=\"/cdn\" /><meta name={name} content=\"x\" /></Head>;\n";
    let found = details(&errors(source, &options));
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found.iter().all(|detail| detail.contains("string literal")), "{found:?}");

    for (tag, count) in [
        ("<meta content=\"x\" />", 1),
        ("<meta name=\"a\" property=\"b\" content=\"x\" />", 1),
        ("<link rel=\"preconnect\" />", 1),
        ("<link href=\"/cdn\" />", 1),
    ] {
        let source = format!(
            "import {{ Head }} from \"reze-js\";\n\nexport const view = <Head>{tag}</Head>;\n"
        );
        let found = details(&errors(&source, &options));
        assert_eq!(found.len(), count, "{tag}: {found:?}");
    }

    for tag in [
        "<script>alert(1)</script>",
        "<script type=\"module\">import \"/a.js\"</script>",
        "<script type={kind}>data</script>",
    ] {
        let source = format!(
            "import {{ Head }} from \"reze-js\";\n\nexport const view = <Head>{tag}</Head>;\n"
        );
        assert!(has(&errors(&source, &options), Code::HeadAttr), "{tag}");
    }
}
