use reze_compiler::{Code, CompileTarget, Options, Severity, compile};

fn code(source: &str, filename: &str, module_id: &str, target: CompileTarget) -> String {
    let options = Options { target, module_id: Some(module_id.into()), ..Options::default() };
    compile(source, filename, &options).expect("compiles").expect("rewrites").code
}

fn seed_key_is_valid(key: &str) -> bool {
    let Some((hash, rest)) = key.split_once('_') else { return false };
    hash.len() == 16
        && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && !rest.is_empty()
        && rest.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
}

#[test]
fn nonclient_targets_require_identity_even_without_transformable_source() {
    for (target, name) in [(CompileTarget::Island, "island"), (CompileTarget::Html, "html")] {
        for module_id in [None, Some(String::new())] {
            for source in ["const value = 1;", "const view = <div />;", "const ="] {
                let options =
                    Options { target, module_id: module_id.clone(), ..Options::default() };
                let errors = compile(source, "test.tsx", &options).err().expect("missing identity");
                assert_eq!(errors.len(), 1);
                let error = &errors[0];
                assert_eq!(error.code, Code::MissingModuleId);
                assert_eq!(error.severity, Severity::Error);
                assert_eq!(error.file, "test.tsx");
                assert_eq!((error.start.offset, error.start.line, error.start.column), (0, 1, 0));
                assert_eq!(error.data.get("target").map(String::as_str), Some(name));
            }
        }
    }
}

#[test]
fn html_and_island_register_components_and_hide_the_non_exported_ones() {
    let source = "import { signal } from \"reze-js\";\nexport function Counter() {\n  let n = signal(0);\n  return <button onClick={() => { n += 1; }}>{n}</button>;\n}";
    let out = code(source, "src/Counter.tsx", "src/Counter.tsx", CompileTarget::Html);
    assert!(
        out.contains("hDefineComponent(Counter, \"src/Counter.tsx\", \"Counter\", 1)"),
        "{out}"
    );

    let source = "export function Static() {\n  return <p>hi</p>;\n}";
    let out = code(source, "src/Static.tsx", "src/Static.tsx", CompileTarget::Html);
    assert!(out.contains("hDefineComponent(Static, \"src/Static.tsx\", \"Static\", 0)"), "{out}");

    let source =
        "function Local() {\n  return <i />;\n}\nexport function Shown() {\n  return <Local />;\n}";
    for target in [CompileTarget::Html, CompileTarget::Island] {
        let out = code(source, "src/Local.tsx", "src/Local.tsx", target);
        assert!(out.contains("export { Local as __rz$Local };"), "{out}");
    }
}

#[test]
fn island_replays_awaits_under_the_continuation_site_key() {
    let source = "export async function Slow() {\n  const v = await load();\n  return <p>{v}</p>;\n}\nSlow.pending = <p>loading</p>;";
    let out = code(source, "src/Slow.tsx", "src/Slow.tsx", CompileTarget::Island);
    let begin = out
        .split("beginContinuation(\"")
        .nth(1)
        .unwrap_or_else(|| panic!("no keyed begin in {out}"));
    let key = begin.split('"').next().unwrap();
    assert!(seed_key_is_valid(key), "{key}");
    assert!(out.contains("suspendLazy(() => load())"), "{out}");
}

#[test]
fn html_keeps_module_state_per_request_without_layout_or_module_markers() {
    let source = "import { signal } from \"reze-js\";\nlet hits = signal(0);\nexport function Hits() {\n  hits += 1;\n  return <p>{hits}</p>;\n}";
    let out = code(source, "src/m.tsx", "src/m.ts", CompileTarget::Html);
    assert!(out.contains("moduleState(\"src/m.ts\""), "{out}");
    assert!(!out.contains("beginModuleScope"), "{out}");
    assert!(!out.contains("withModuleScope"), "{out}");
    assert!(!out.contains("layout"), "{out}");
    assert!(!out.contains("markModule"), "{out}");
}

#[test]
fn html_exports_contexts_as_callable_module_proxies() {
    let source = "import { createContext, store } from \"reze-js\";\nexport const Theme = createContext(\"light\");\nexport const settings = store({ dense: false });";
    let out = code(source, "src/m.tsx", "src/m.ts", CompileTarget::Html);
    assert!(out.contains("().Theme, true)"), "{out}");
    assert!(out.contains("().settings)"), "{out}");
}

#[test]
fn html_compiles_views_rooted_at_html_as_documents() {
    let source = "export default (props) => <html><head><title>{props.title}</title></head><body><main>{props.children}</main></body></html>;";
    let out = code(source, "src/document.tsx", "src/document.tsx", CompileTarget::Html);
    assert!(out.contains("_$hRoot(\"html\", \"\", _site$)"), "{out}");

    for target in [CompileTarget::Client, CompileTarget::Island] {
        let options =
            Options { target, module_id: Some("src/document.tsx".into()), ..Options::default() };
        let errors = compile(source, "src/document.tsx", &options).err().expect("relocates");
        assert_eq!(errors[0].code, Code::ParserRelocation);
    }
}

#[test]
fn html_writes_svg_inner_html_attributes_as_markup() {
    let source = "export function Icon(p) {\n  return <svg><g innerHTML={p.a} /></svg>;\n}";
    let out = code(source, "src/Icon.tsx", "src/Icon.tsx", CompileTarget::Html);
    assert!(out.contains("hSetInnerHTML("), "{out}");
    assert!(!out.contains("\"innerHTML\""), "{out}");
}
