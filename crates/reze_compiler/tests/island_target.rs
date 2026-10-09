use reze_compiler::{CompileTarget, Options, compile};

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
fn html_registers_an_exported_component_with_client_work() {
    let source = "import { signal } from \"reze-js\";\nexport function Counter() {\n  let n = signal(0);\n  return <button onClick={() => { n += 1; }}>{n}</button>;\n}";
    let out = code(source, "src/Counter.tsx", "src/Counter.tsx", CompileTarget::Html);
    assert!(out.contains("hDefineComponent(Counter, \"src/Counter.tsx\", \"Counter\", 1)"), "{out}");
}

#[test]
fn html_registers_a_static_component_without_client_work() {
    let source = "export function Static() {\n  return <p>hi</p>;\n}";
    let out = code(source, "src/Static.tsx", "src/Static.tsx", CompileTarget::Html);
    assert!(out.contains("hDefineComponent(Static, \"src/Static.tsx\", \"Static\", 0)"), "{out}");
}

#[test]
fn non_exported_components_get_a_hidden_export_in_html_and_island() {
    let source = "function Local() {\n  return <i />;\n}\nexport function Shown() {\n  return <Local />;\n}";
    for target in [CompileTarget::Html, CompileTarget::Island] {
        let out = code(source, "src/Local.tsx", "src/Local.tsx", target);
        assert!(out.contains("export { Local as __rz$Local };"), "{out}");
    }
}

#[test]
fn island_replays_awaits_under_the_continuation_site_key() {
    let source = "export async function Slow() {\n  const v = await load();\n  return <p>{v}</p>;\n}\nSlow.pending = <p>loading</p>;";
    let out = code(source, "src/Slow.tsx", "src/Slow.tsx", CompileTarget::Island);
    let begin = out.split("beginContinuation(\"").nth(1).unwrap_or_else(|| panic!("no keyed begin in {out}"));
    let key = begin.split('"').next().unwrap();
    assert!(seed_key_is_valid(key), "{key}");
    assert!(out.contains("suspendLazy(() => load())"), "{out}");
}

#[test]
fn html_keeps_module_state_per_request() {
    let source = "import { signal } from \"reze-js\";\nlet hits = signal(0);\nexport function Hits() {\n  hits += 1;\n  return <p>{hits}</p>;\n}";
    let out = code(source, "src/m.tsx", "src/m.ts", CompileTarget::Html);
    assert!(out.contains("moduleState(\"src/m.ts\""), "{out}");
    assert!(!out.contains("beginModuleScope"), "{out}");
    assert!(!out.contains("withModuleScope"), "{out}");
}

#[test]
fn html_sites_carry_no_layout_and_no_module_marker() {
    let source = "export function Counter() {\n  return <p>hi</p>;\n}";
    let out = code(source, "src/Counter.tsx", "src/Counter.tsx", CompileTarget::Html);
    assert!(!out.contains("layout"), "{out}");
    assert!(!out.contains("markModule"), "{out}");
}
