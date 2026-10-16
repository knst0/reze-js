use reze_compiler::{Code, Diagnostic, compile_document};

fn render(source: &str) -> (String, String) {
    let document =
        compile_document(source, "document.tsx").unwrap_or_else(|errors| panic!("{errors:#?}"));
    (document.html, document.root_id)
}

fn errors(source: &str) -> Vec<Diagnostic> {
    match compile_document(source, "document.tsx") {
        Ok(document) => panic!("expected errors, rendered {}", document.html),
        Err(errors) => errors,
    }
}

fn details(source: &str, code: Code) -> Vec<String> {
    errors(source)
        .iter()
        .filter(|d| d.code == code)
        .map(|d| d.data.get("detail").expect("detail").clone())
        .collect()
}

#[test]
fn renders_a_static_page_with_an_empty_root() {
    let (html, root_id) = render(
        r##"import type { JSX } from "reze-js";

export default function Document(props: { children: JSX.Element }) {
  return (
    <html lang="ru">
      <head>
        <meta charset="utf-8" />
        <meta name="theme-color" media="(prefers-color-scheme: dark)" content="#1d1e20" />
        <title>Fish &amp; "chips"</title>
        <link rel="icon" href={"/favicon.svg"} />
        <script>{`navigator.serviceWorker?.register("/sw.js") && 1 < 2;`}</script>
      </head>
      <body class="min-h-dvh">
        <div id="root">{props.children}</div>
        <noscript>Enable JavaScript</noscript>
      </body>
    </html>
  );
}
"##,
    );
    assert_eq!(root_id, "root");
    assert_eq!(
        html,
        "<!doctype html><html lang=ru><head><meta charset=utf-8><meta name=theme-color media=\"(prefers-color-scheme: dark)\" content=#1d1e20><title>Fish &amp; \"chips\"</title><link rel=icon href=/favicon.svg><script>navigator.serviceWorker?.register(\"/sw.js\") && 1 < 2;</script></head><body class=min-h-dvh><div id=root></div><noscript>Enable JavaScript</noscript></body></html>"
    );
}

#[test]
fn defaults_the_root_id_and_accepts_arrows_and_destructured_children() {
    let (html, root_id) = render(
        "export default ({ children }) => <html><head /><body><main hidden>{children}</main></body></html>;\n",
    );
    assert_eq!(root_id, "app");
    assert_eq!(
        html,
        "<!doctype html><html><head></head><body><main hidden id=app></main></body></html>"
    );
}

#[test]
fn refuses_dynamic_values() {
    let found = details(
        "import { Theme } from \"./theme\";\n\nexport default function Document(props) {\n  return <html lang={navigator.language}><head /><body {...props}><Theme /><div>{props.children}</div></body></html>;\n}\n",
        Code::DocumentDynamic,
    );
    assert_eq!(found, ["an attribute expression", "a spread attribute", "a component"]);
    assert!(errors("import \"./styles.css\";\n\nexport default function Document(props) {\n  return <html><head /><body><div>{props.children}</div></body></html>;\n}\n")
        .iter()
        .any(|d| d.code == Code::DocumentShape));
}

#[test]
fn refuses_pages_that_are_not_html_with_head_and_body() {
    assert_eq!(
        details(
            "export default function Document(props) {\n  return <div>{props.children}</div>;\n}\n",
            Code::DocumentShape
        ),
        ["returns something other than an `<html>` element"]
    );
    assert_eq!(
        details(
            "export default function Document(props) {\n  return <html><body><div>{props.children}</div></body></html>;\n}\n",
            Code::DocumentShape
        ),
        ["`<html>` holds `<head>` and then `<body>`"]
    );
    assert_eq!(
        details(
            "export default function Document(props) {\n  const lang = \"en\";\n  return <html lang={lang}><head /><body><div>{props.children}</div></body></html>;\n}\n",
            Code::DocumentShape
        ),
        ["has statements besides `return <html>…</html>`"]
    );
    assert_eq!(
        details(
            "export default function Document(props) {\n  return <html><head><script>{\"</script><b>\"}</script></head><body><div>{props.children}</div></body></html>;\n}\n",
            Code::DocumentShape
        ),
        ["puts the closing tag text inside a raw-text element"]
    );
}

#[test]
fn refuses_misplaced_roots() {
    let root = |body: &str| {
        details(
            &format!(
                "export default function Document(props) {{\n  return <html><head /><body>{body}</body></html>;\n}}\n"
            ),
            Code::DocumentRoot,
        )
    };
    assert_eq!(
        root("<div />"),
        ["never renders its `children`; place them in the element the app renders into"]
    );
    assert_eq!(
        root("<div><p />{props.children}</div>"),
        ["renders its `children` next to other content; give them an element of their own"]
    );
    assert_eq!(
        root("<div>{props.children}</div><div>{props.children}</div>"),
        ["renders its `children` more than once"]
    );
    assert_eq!(
        root("<div id=\"app\" /><main>{props.children}</main>"),
        ["gives another element the root element's `id`"]
    );
    assert_eq!(root("<template>{props.children}</template>").len(), 1);
}
