use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use reze_compiler::{Options, compile};

const CASES: &[(&str, &str)] = &[
    ("static_template", "const a = <div class=\"box\"><p>hi</p><br /></div>;"),
    ("text_insert_marker", "const a = <p>hi {name()}!</p>;"),
    (
        "attributes",
        "const a = <a href={url()} title=\"t\" data-id={id} aria-label={label()} bool:hidden={h()} attr:x=\"1\" prop:foo={v} xlink:href=\"#i\" />;",
    ),
    (
        "merged_binds",
        "const a = <input value={v()} checked={c()} style={{ color: color() }} class={cls()} />;",
    ),
    ("class_sources", "const a = <i class=\"a\" className=\"b\" classList={{ on: on() }} />;"),
    (
        "static_class_and_style",
        "const a = <i class={[\"a\", { b: true, c: false }]} style={{ color: \"red\" }} />;",
    ),
    (
        "events",
        "const a = <div onClick={() => go()} onInput={[pick, 1]} onKeyDown={handler} on:scroll={s} onDoubleClick={d} onclick=\"legacy()\" />;",
    ),
    (
        "refs",
        "let el; const a = <div ref={el}><b ref={(b) => use(b)} /><i ref={refs[i++]} /><u ref={pick()} /></div>;",
    ),
    (
        "component_props",
        "const a = <Card title=\"t\" count={n()} static={s} {...rest} onPick={() => pick()} ref={box.el}>body {n()}</Card>;",
    ),
    ("component_dynamic_spread", "const a = <Card {...props()} a={1} />;"),
    ("native_spread", "const a = <div {...attrs} className=\"x\" ref={el}>{kids()}</div>;"),
    (
        "conditionals",
        "const a = <div>{ok() ? <b>yes</b> : <i>no</i>}{open() && <p>{text()}</p>}</div>;",
    ),
    ("fragments", "const a = <>{a()}<b />text</>;\nconst b = <></>;\nconst c = <>{x}</>;"),
    (
        "svg_and_math",
        "const a = <svg viewBox=\"0 0 1 1\"><circle r={r()} /><foreignObject><p>x</p></foreignObject></svg>;\nconst b = <g><path d=\"M0\" /></g>;\nconst c = <math><mi>x</mi></math>;",
    ),
    (
        "select_and_textarea",
        "const a = <select value={v()}>{options()}</select>;\nconst b = <textarea value=\"hi\" />;",
    ),
    (
        "children_attribute",
        "const a = <div children={kids()} />;\nconst b = <div children={x()}><b /></div>;",
    ),
    (
        "async_component",
        "async function User(props: { id: number }): Promise<JSX.Element> {\n  const user = await fetchUser(props.id);\n  const posts = await fetchPosts(user.id);\n  return <ul>{posts.map((p) => <li>{p.title}</li>)}</ul>;\n}",
    ),
    (
        "async_rejected",
        "async function User(props) {\n  const label = props.label;\n  const user = await fetchUser(props.id);\n  return <p>{label}{user.name}</p>;\n}",
    ),
    (
        "const_signal",
        "import { signal } from \"reze-js\";\nconst [title] = signal(\"Reze\");\nconst [count, setCount] = signal(0);\nexport const a = <h1 onClick={() => setCount(count() + 1)}>{title()}: {count()}</h1>;",
    ),
    (
        "dead_branches",
        "const DEBUG = false;\nconst a = <div>{false && <b>never</b>}{true ? <i>y</i> : <u>n</u>}{DEBUG && <p />}</div>;",
    ),
    (
        "warnings",
        "import { signal } from \"reze-js\";\nconst [n, setN] = signal(0); setN(1);\nfunction Greeting({ name }) {\n  return <p key=\"k\" clas=\"x\" title={n} title=\"y\">{name}<For each={[1, 2]}>{(i) => i}</For></p>;\n}",
    ),
];

fn render(source: &str, optimize: bool) -> String {
    let options = Options { source_map: false, optimize, ..Options::default() };
    let out = match compile(source, "case.tsx", &options) {
        Ok(Some(out)) => out,
        Ok(None) => return "<no JSX>".to_string(),
        Err(errors) => {
            return errors.iter().map(|d| d.rendered.clone()).collect::<Vec<_>>().join("\n\n");
        }
    };
    assert_valid(&out.code);
    let mut text = out.code;
    for diagnostic in &out.diagnostics {
        text.push_str("\n// ");
        text.push_str(diagnostic.severity.as_str());
        text.push('\n');
        text.push_str(&diagnostic.rendered);
    }
    text
}

fn assert_valid(code: &str) {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, SourceType::tsx()).parse();
    assert!(parsed.diagnostics.is_empty(), "{:?}\n{code}", parsed.diagnostics);
    let semantic = SemanticBuilder::new().with_check_syntax_error(true).build(&parsed.program);
    assert!(semantic.diagnostics.is_empty(), "{:?}\n{code}", semantic.diagnostics);
}

#[test]
fn output_snapshots() {
    for (name, source) in CASES {
        insta::assert_snapshot!(*name, render(source, true), source);
    }
}

#[test]
fn unoptimized_output_is_valid_too() {
    for (_, source) in CASES {
        render(source, false);
    }
}
