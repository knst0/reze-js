use reze_compiler::{Code, Diagnostic, DocumentModules, DocumentOutcome, compile_document};

fn compile(
    source: &str,
    modules: &[(&str, &str)],
    defines: &[(&str, &str)],
) -> Result<DocumentOutcome, Vec<Diagnostic>> {
    let mut input = DocumentModules::default();
    for (file, module) in modules {
        input.add_source(file.to_string(), module.to_string());
    }
    for (key, code) in defines {
        input.define(key.to_string(), code.to_string());
    }
    loop {
        match compile_document(source, "document.tsx", &input) {
            Ok(DocumentOutcome::Resolve(requests)) => {
                for request in requests {
                    let file = modules
                        .iter()
                        .find(|(file, _)| *file == request.specifier)
                        .map(|(file, _)| file.to_string());
                    input.resolve(request, file);
                }
            }
            outcome => return outcome,
        }
    }
}

fn render_with(source: &str, modules: &[(&str, &str)]) -> (String, String) {
    render_defined(source, modules, &[])
}

fn render_defined(
    source: &str,
    modules: &[(&str, &str)],
    defines: &[(&str, &str)],
) -> (String, String) {
    match compile(source, modules, defines) {
        Ok(DocumentOutcome::Html(document)) => (document.html, document.root_id),
        Ok(DocumentOutcome::Render(reasons)) => panic!("expected a static page, got {reasons:#?}"),
        Ok(DocumentOutcome::Resolve(_)) => unreachable!(),
        Err(errors) => panic!("{errors:#?}"),
    }
}

fn render(source: &str) -> (String, String) {
    render_with(source, &[])
}

fn errors(source: &str) -> Vec<Diagnostic> {
    match compile(source, &[], &[]) {
        Err(errors) => errors,
        Ok(DocumentOutcome::Html(document)) => {
            panic!("expected errors, rendered {}", document.html)
        }
        Ok(_) => panic!("expected errors"),
    }
}

fn details(source: &str, code: Code) -> Vec<String> {
    errors(source)
        .iter()
        .filter(|d| d.code == code)
        .map(|d| d.data.get("detail").expect("detail").clone())
        .collect()
}

fn reasons_with(source: &str, modules: &[(&str, &str)]) -> Vec<(String, String)> {
    reasons_defined(source, modules, &[])
}

fn reasons_defined(
    source: &str,
    modules: &[(&str, &str)],
    defines: &[(&str, &str)],
) -> Vec<(String, String)> {
    match compile(source, modules, defines) {
        Ok(DocumentOutcome::Render(reasons)) => reasons
            .iter()
            .map(|d| (d.file.clone(), d.data.get("detail").expect("detail").clone()))
            .collect(),
        Ok(DocumentOutcome::Html(document)) => panic!("expected a render, got {}", document.html),
        Ok(DocumentOutcome::Resolve(_)) => unreachable!(),
        Err(errors) => panic!("{errors:#?}"),
    }
}

fn reasons(source: &str) -> Vec<String> {
    reasons_with(source, &[]).into_iter().map(|(_, detail)| detail).collect()
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
fn renders_pages_that_are_not_static_by_running_them() {
    assert_eq!(
        reasons(
            "import { Theme } from \"@acme/theme\";\n\nexport default function Document(props) {\n  return <html lang={navigator.language}><head /><body {...props}><Theme /><div>{props.children}</div></body></html>;\n}\n"
        ),
        ["an attribute expression", "a spread attribute"]
    );
    assert_eq!(
        reasons_with(
            "import { Theme } from \"@acme/theme\";\n\nexport default function Document(props) {\n  return <html><head><Theme /></head><body><div>{props.children}</div></body></html>;\n}\n",
            &[]
        ),
        [(
            "document.tsx".to_string(),
            "`Theme` from `@acme/theme`, which is not a source module".to_string()
        )]
    );
    assert_eq!(
        reasons(
            "import \"./polyfill\";\nlet year = 2025;\n\nexport default function Document(props) {\n  const lang = \"en\";\n  return <html lang={lang}><head /><body><div>{props.children}</div></body></html>;\n}\n"
        ),
        [
            "a runtime statement",
            "a runtime statement",
            "statements besides `return <html>…</html>`"
        ]
    );
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

#[test]
fn inlines_local_components_with_static_props_and_children() {
    let (html, root_id) = render(
        r##"import type { JSX } from "reze-js";

function Meta({ name, content = "none" }: { name: string; content?: string }) {
  return <meta name={name} content={content} />;
}

const Icons = () => (
  <>
    <link rel="icon" href="/favicon.svg" />
    <link rel="apple-touch-icon" href="/icon.png" />
  </>
);

const Inline = (props: { children: string }) => <script>{props.children}</script>;

function Main(props: { children: JSX.Element; hidden?: boolean }) {
  return <main id="root" hidden={props.hidden} class={props.class}>{props.children}</main>;
}

function Shell(props) {
  return <Main hidden>{props.children}</Main>;
}

export default function Document(props: { children: JSX.Element }) {
  return (
    <html>
      <head>
        <Meta name="robots" content="index" />
        <Meta name="color-scheme" />
        <Icons />
        <Inline>{`if (1 < 2) document.documentElement.dataset.js = "";`}</Inline>
      </head>
      <body>
        <Shell>{props.children}</Shell>
      </body>
    </html>
  );
}
"##,
    );
    assert_eq!(root_id, "root");
    assert_eq!(
        html,
        "<!doctype html><html><head><meta name=robots content=index><meta name=color-scheme content=none><link rel=icon href=/favicon.svg><link rel=apple-touch-icon href=/icon.png><script>if (1 < 2) document.documentElement.dataset.js = \"\";</script></head><body><main id=root hidden></main></body></html>"
    );
}

#[test]
fn renders_components_it_cannot_inline_by_running_them() {
    let page = |components: &str, body: &str| {
        format!(
            "{components}\nexport default function Document(props) {{\n  return <html><head /><body>{body}<div>{{props.children}}</div></body></html>;\n}}\n"
        )
    };
    assert_eq!(
        details(
            &page("function Loop() {\n  return <p><Loop /></p>;\n}\n", "<Loop />"),
            Code::DocumentShape
        ),
        ["renders `Loop` inside itself"]
    );
    assert_eq!(
        reasons(&page("async function Late() {\n  return <p />;\n}\n", "<Late />")),
        ["`Late`, which is async or a generator"]
    );
    assert_eq!(
        reasons(&page(
            "function Year({ year = new Date().getFullYear() }) {\n  return <p>{year}</p>;\n}\n",
            "<Year />"
        )),
        ["a prop default other than a literal"]
    );
    let found = match compile(
        &page(
            "function Label(props) {\n  return <p title={props.title}>{props.text.trim()}</p>;\n}\n",
            "<Label title={navigator.language} />",
        ),
        &[],
        &[],
    ) {
        Ok(DocumentOutcome::Render(reasons)) => reasons
            .iter()
            .map(|d| (d.data.get("detail").expect("detail").clone(), d.path.clone()))
            .collect::<Vec<_>>(),
        _ => panic!("expected a render"),
    };
    assert_eq!(
        found,
        [
            ("an expression".to_string(), vec!["<Label>".to_string()]),
            ("an attribute expression".to_string(), vec![])
        ]
    );
    assert_eq!(
        details(
            &page(
                "function Twice(props) {\n  return <div>{props.children}</div>;\n}\n",
                "<Twice>{props.children}</Twice>"
            ),
            Code::DocumentRoot
        ),
        ["renders its `children` more than once"]
    );
}

#[test]
fn inlines_imported_components_and_constants() {
    let modules = [
        (
            "@acme/head",
            "export * from \"./preload\";\nexport { Shell as default } from \"./shell\";\n",
        ),
        (
            "./preload",
            "import { kind } from \"./kind\";\nimport { trace } from \"./trace\";\n\ntrace();\n\nexport function Preload(props: { href: string }) {\n  return <link rel=\"preload\" as={kind} href={props.href} crossorigin />;\n}\n\nexport function helper() {\n  return trace();\n}\n",
        ),
        ("./kind", "export const kind = \"font\";\n"),
        ("./shell", "export const Shell = (props) => <main id=\"root\">{props.children}</main>;\n"),
        ("./site", "export const title = `Acme`;\nexport default \"en\";\n"),
    ];
    let (html, root_id) = render_with(
        "import Shell, { Preload } from \"@acme/head\";\nimport lang, { title } from \"./site\";\nimport type { JSX } from \"reze-js\";\n\nexport default function Document(props: { children: JSX.Element }) {\n  return (\n    <html lang={lang}>\n      <head>\n        <title>{title}</title>\n        <Preload href=\"/inter.woff2\" />\n      </head>\n      <body>\n        <Shell>{props.children}</Shell>\n      </body>\n    </html>\n  );\n}\n",
        &modules,
    );
    assert_eq!(root_id, "root");
    assert_eq!(
        html,
        "<!doctype html><html lang=en><head><title>Acme</title><link rel=preload as=font href=/inter.woff2 crossorigin></head><body><main id=root></main></body></html>"
    );
    assert_eq!(
        reasons_with(
            "import { Preload } from \"./preload\";\n\nexport default function Document(props) {\n  return <html><head><Preload href=\"/a.woff2\" /></head><body><div>{props.children}</div></body></html>;\n}\n",
            &[(
                "./preload",
                "import { pick } from \"./pick\";\n\nexport function Preload(props) {\n  return <link rel=\"preload\" href={pick(props.href)} />;\n}\n",
            )],
        ),
        [("./preload".to_string(), "an attribute expression".to_string())]
    );
}

const DEFINES: &[(&str, &str)] = &[
    ("__META__", r##"{ "siteName": "Reze", "theme": { "color": "#1d1e20" } }"##),
    ("__META__.theme.color", "\"#000\""),
    ("import.meta.env.LANG", r#""ru""#),
    ("__DEV__", "false"),
    ("__BROKEN__", "{"),
];

#[test]
fn inlines_defines_the_bundler_replaces() {
    let (html, _) = render_defined(
        r#"import { title } from "./meta";
import { __DEV__ } from "./flags";

const name = __META__.siteName;

export default function Document(props) {
  return (
    <html lang={import.meta.env.LANG}>
      <head>
        <title>{name} – {title}</title>
        <meta name="theme-color" content={__META__.theme.color} />
      </head>
      <body hidden={__DEV__}><div>{props.children}</div></body>
    </html>
  );
}
"#,
        &[
            ("./meta", "export const title = (__META__).siteName;\n"),
            ("./flags", "export const __DEV__ = true;\n"),
        ],
        DEFINES,
    );
    assert_eq!(
        html,
        "<!doctype html><html lang=ru><head><title>Reze – Reze</title><meta name=theme-color content=#000></head><body hidden><div id=app></div></body></html>"
    );
}

#[test]
fn runs_defines_it_cannot_read_or_that_a_binding_shadows() {
    let reasons: Vec<String> = reasons_defined(
        r#"export default function Document({ __META__, children }) {
  return (
    <html>
      <head>
        <title>{__META__.siteName}</title>
        <meta name="b" content={__BROKEN__} />
      </head>
      <body><div>{children}</div></body>
    </html>
  );
}
"#,
        &[],
        DEFINES,
    )
    .into_iter()
    .map(|(_, detail)| detail)
    .collect();
    assert_eq!(
        reasons,
["an expression", "an attribute expression"]
    );
    let reasons: Vec<String> = reasons_defined(
        "export default function Document(props) {\n  return <html><head><title>{__META__.missing}</title><meta content={__META__.theme} /></head><body><div>{props.children}</div></body></html>;\n}\n",
        &[],
        DEFINES,
    )
    .into_iter()
    .map(|(_, detail)| detail)
    .collect();
    assert_eq!(reasons, ["an expression", "an attribute expression"]);
}
