use reze_compiler::{Options, ProfileComponent, ProfileFacts, compile, profile_hash};

fn options() -> Options {
    Options { source_map: false, ..Options::default() }
}

fn compiled(source: &str, options: &Options) -> String {
    match compile(source, "test.tsx", options).expect("compiles") {
        Some(out) => out.code,
        None => source.to_string(),
    }
}

fn effects(code: &str) -> usize {
    code.matches("renderEffect").count()
}

const SOURCE: &str = "import { signal } from \"reze-js\";\nconst [a, setA] = signal(0);\nconst [b, setB] = signal(0);\nexport const view = <p title={a()} data-x={b()} onClick={() => { setA(1); setB(2); }}>hi</p>;";
const COMPONENT: &str = "export function App() { return <Card title=\"hi\" />; }";

fn facts(source: &str, reruns: u32) -> Options {
    Options {
        profile: Some(ProfileFacts {
            hash: profile_hash(source),
            components: vec![ProfileComponent {
                component: "view".into(),
                file: "test.tsx".into(),
                mounts: 1,
                props: 0,
                reruns,
                writes: 2,
            }],
        }),
        ..options()
    }
}

#[test]
fn profile_hash_is_fnv1a64() {
    assert_eq!(profile_hash(""), "cbf29ce484222325");
}

#[test]
fn cold_facts_merge_bind_groups_into_one_effect() {
    assert_eq!(effects(&compiled(SOURCE, &options())), 4);
    assert_eq!(effects(&compiled(SOURCE, &facts(SOURCE, 0))), 3);
}

#[test]
fn rerun_facts_keep_static_grouping() {
    assert_eq!(effects(&compiled(SOURCE, &facts(SOURCE, 4))), 4);
}

#[test]
fn stale_hash_compiles_as_without_facts() {
    let mut stale = facts(SOURCE, 0);
    stale.profile.as_mut().expect("facts").hash = "0000000000000000".into();
    assert_eq!(effects(&compiled(SOURCE, &stale)), 4);
}

#[test]
fn debug_names_attribute_components_with_file_and_tag() {
    let debug = Options { debug_names: true, ..options() };
    let code = compiled(COMPONENT, &debug);
    assert!(code.contains("\"test.tsx#Card\""), "{code}");
    assert!(!compiled(COMPONENT, &options()).contains('#'), "{code}");
}
