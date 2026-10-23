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
fn identities_read_from_string_constants_are_folded() {
    let source = "import { Head } from \"reze-js\";\n\nconst CDN = `https://cdn.example`;\nconst SDK = `${CDN}/sdk.js` as const;\nconst REL = \"preconnect\";\n\nexport function View() {\n  const KIND = \"application/ld+json\";\n  return <Head><script src={SDK} async /><link rel={REL} href={CDN} /><meta name={REL} content=\"x\" /><script type={KIND}>{\"{}\"}</script></Head>;\n}\n";
    for target in [CompileTarget::Client, CompileTarget::Island, CompileTarget::Html] {
        let code = code_for(source, target);
        for key in [
            "\"script:https://cdn.example/sdk.js\"",
            "\"link:preconnect\\nhttps://cdn.example\"",
            "\"meta:name:preconnect\"",
        ] {
            assert!(code.contains(key), "{key}: {code}");
        }
    }
}

#[test]
fn unresolved_identities_explain_why() {
    let options = without_source_map();
    let source = "import { Head } from \"reze-js\";\nimport { SDK } from \"./urls\";\n\nlet mutable = \"/a.js\";\nconst computed = location.origin;\nconst LATE = `${LATER}/x.js`;\nconst LATER = \"/cdn\";\n\nexport const view = <Head><script src={SDK} /><script src={mutable} /><script src={computed} /><script src={LATE} /><script src={pick()} /></Head>;\n";
    let found = details(&errors(source, &options));
    assert_eq!(found.len(), 5, "{found:?}");
    assert!(
        found[0].contains("`SDK` is imported and imported bindings are not resolved"),
        "{found:?}"
    );
    assert!(found[1].contains("`mutable` is not a `const` holding a string"), "{found:?}");
    assert!(found[2].contains("`computed` is not a `const` holding a string"), "{found:?}");
    assert!(found[3].contains("`LATE` is not a `const` holding a string"), "{found:?}");
    assert!(found[4].contains("as a string literal or a `const` holding one"), "{found:?}");
}

#[test]
fn every_head_element_compiles_with_its_standard_key() {
    let source = "import { Head, signal } from \"reze-js\";\n\nlet page = signal(\"a\");\n\nexport const view = <Head><title>{page} | Site</title><base href=\"/app/\" /><meta http-equiv=\"content-security-policy\" content=\"default-src 'self'\" /><link rel=\"canonical\" href={`/p/${page}`} /><style>{\"body{margin:0}\"}</style></Head>;\n";
    for target in [CompileTarget::Client, CompileTarget::Html] {
        let code = code_for(source, target);
        for key in [
            "\"title\"",
            "\"base\"",
            "\"meta:http-equiv:content-security-policy\"",
            "\"link:canonical\"",
        ] {
            assert!(code.contains(key), "{key}: {code}");
        }
    }
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
        "<template />",
        "<noscript />",
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
    let source = "import { Head } from \"reze-js\";\n\nexport const view = <Head><script src=\"/a.js\"><b>bold</b></script><meta name=\"a\" content=\"b\">text</meta><title><b>bold</b></title><style><i /></style></Head>;\n";
    let found = details(&errors(source, &options));
    assert_eq!(found.len(), 4, "{found:?}");
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
    assert!(
        found.iter().all(|detail| detail.contains("is not a `const` holding a string")),
        "{found:?}"
    );

    for (tag, count) in [
        ("<meta content=\"x\" />", 1),
        ("<meta name=\"a\" property=\"b\" content=\"x\" />", 1),
        ("<meta charset=\"utf-8\" />", 1),
        ("<link rel={rel} href=\"/cdn\" />", 1),
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
