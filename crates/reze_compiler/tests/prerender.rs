use reze_compiler::{Options, Output, PrerenderModule, Tree, compile};

fn prerender(source: &str) -> PrerenderModule {
    let options = Options { prerender: true, ..Options::default() };
    let out: Output = compile(source, "test.tsx", &options).expect("compiles").expect("rewrites");
    out.prerender.expect("prerender collected")
}

fn component(source: &str, name: &str) -> Tree {
    let module = prerender(source);
    module
        .components
        .iter()
        .find(|component| component.name == name)
        .unwrap_or_else(|| panic!("no component {name}"))
        .tree
        .clone()
}

fn html(tree: &Tree) -> &str {
    match tree {
        Tree::Html { html, .. } => html,
        tree => panic!("not html: {tree:?}"),
    }
}

#[test]
fn off_by_default() {
    let out = compile(
        "export function Card() {\n  return <p>hi</p>;\n}\n",
        "test.tsx",
        &Options::default(),
    )
    .expect("compiles")
    .expect("rewrites");
    assert!(out.prerender.is_none());
}

#[test]
fn static_component_renders_full_tags() {
    let tree = component(
        "export function Card() {\n  return <article class=\"card\"><h1>hi</h1></article>;\n}\n",
        "Card",
    );
    assert_eq!(html(&tree), "<article class=card><h1>hi</h1></article>");
}

#[test]
fn static_natives_nest() {
    let tree = component(
        "export function Page() {\n  return <main><h1>title</h1><p>text</p></main>;\n}\n",
        "Page",
    );
    assert_eq!(html(&tree), "<main><h1>title</h1><p>text</p></main>");
}

#[test]
fn dynamic_text_renders_empty() {
    let tree = component(
        "import { $signal } from \"reze-js\";\n\nexport function Counter() {\n  let count = $signal(0);\n  const inc = () => (count += 1);\n  return <output onClick={inc}>{count}</output>;\n}\n",
        "Counter",
    );
    assert_eq!(html(&tree), "<output></output>");
}

#[test]
fn folded_constants_render() {
    let tree = component(
        "import { $signal } from \"reze-js\";\n\nexport function Counter() {\n  let count = $signal(0);\n  return <output>{count}</output>;\n}\n",
        "Counter",
    );
    assert_eq!(html(&tree), "<output>0</output>");
}

#[test]
fn folded_boolean_attributes_preserve_truthiness() {
    for (expression, enabled) in
        [("-0", false), ("+0", false), ("+(1 - 1)", false), ("-1", true), ("\"0\"", true)]
    {
        let tree = component(
            &format!(
                "export function Control() {{ return <div><button bool:disabled={{{expression}}}/><input checked={{{expression}}}/><option selected={{{expression}}}/></div>; }}"
            ),
            "Control",
        );
        let rendered = html(&tree);
        for name in ["disabled", "checked", "selected"] {
            assert_eq!(rendered.contains(name), enabled, "{expression}: {rendered}");
        }
    }
}

#[test]
fn nested_component_is_a_hole() {
    let tree = component(
        "import { Counter } from \"./Counter\";\n\nexport function Page() {\n  return <main><h1>title</h1><Counter step={1} /></main>;\n}\n",
        "Page",
    );
    match tree {
        Tree::Mixed { html, holes } => {
            assert_eq!(html, "<main><h1>title</h1><!--reze0--></main>");
            assert_eq!(holes.len(), 1);
            assert_eq!(holes[0].id, 0);
            assert_eq!(holes[0].target.request.as_deref(), Some("./Counter"));
            assert_eq!(holes[0].target.path, vec!["Counter"]);
        }
        tree => panic!("not mixed: {tree:?}"),
    }
}

#[test]
fn local_component_is_a_samemodule_hole() {
    let tree = component(
        "function Counter() {\n  return <b>count</b>;\n}\n\nexport function Page() {\n  return <main><Counter /></main>;\n}\n",
        "Page",
    );
    match tree {
        Tree::Mixed { holes, .. } => {
            assert_eq!(holes.len(), 1);
            assert_eq!(holes[0].target.request, None);
            assert_eq!(holes[0].target.path, vec!["Counter"]);
        }
        tree => panic!("not mixed: {tree:?}"),
    }
}

#[test]
fn island_renders_its_fallback() {
    let tree = component(
        "import { Counter } from \"./Counter\";\n\nexport function Page() {\n  return <main><Counter island=\"visible\" islandFallback={<p>soon</p>} /></main>;\n}\n",
        "Page",
    );
    assert_eq!(html(&tree), "<main><p>soon</p></main>");
}

#[test]
fn render_call_is_a_root() {
    let module = prerender(
        "import { render } from \"reze-js\";\nimport { Counter } from \"./Counter\";\n\nrender(() => <Counter step={1} />, document.body);\n",
    );
    assert_eq!(module.roots.len(), 1);
    match &module.roots[0] {
        Tree::Component(target) => {
            assert_eq!(target.request.as_deref(), Some("./Counter"));
            assert_eq!(target.path, vec!["Counter"]);
        }
        tree => panic!("not a component: {tree:?}"),
    }
}

#[test]
fn default_export_carries_its_name() {
    let module = prerender("export default function Page() {\n  return <main>hi</main>;\n}\n");
    assert_eq!(module.components.len(), 1);
    assert_eq!(module.components[0].exported, vec!["default"]);
    assert_eq!(html(&module.components[0].tree), "<main>hi</main>");
}

#[test]
fn loading_renders_its_fallback() {
    let tree = component(
        "import { Loading } from \"reze-js\";\n\nexport function Page() {\n  return <Loading fallback={<p>wait</p>}><b>late</b></Loading>;\n}\n",
        "Page",
    );
    assert_eq!(html(&tree), "<p>wait</p>");
}

#[test]
fn show_without_a_literal_renders_nothing() {
    let tree = component(
        "import { Show } from \"reze-js\";\n\nexport function Page(props) {\n  return <Show when={props.ok}><b>yes</b></Show>;\n}\n",
        "Page",
    );
    assert_eq!(tree, Tree::Empty);
}

#[test]
fn static_spread_dissolves() {
    let tree = component(
        "export function Page() {\n  return <main {...{ id: \"app\", hidden: true }}>hi</main>;\n}\n",
        "Page",
    );
    assert_eq!(html(&tree), "<main id=app hidden>hi</main>");
}
