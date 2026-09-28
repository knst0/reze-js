use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use reze_compiler::{Options, compile};

const CASES: &[(&str, &str)] = &[
    ("static_template", "const a = <div class=\"box\"><p>hi</p><br /></div>;"),
    ("text_insert_marker", "const a = <p>hi {name()}!</p>;"),
    (
        "text_runs",
        "const a = <p>doubled: {n() * 2}</p>;\nconst b = <p>{n() + 1}</p>;\nconst c = <p>{`${name()}`}</p>;\nconst d = <p>a {\"<\"} b &amp; {n() - 1} items</p>;\nconst e = <div>{n() * 2}<b />{x()}total: {n() % 3}<i />{(n() | 0) + 1}{-n()}</div>;\nconst f = <p>{label()}: {n() / 2}</p>;\nconst g = <p>size {SIZE * 2}</p>;\nconst h = <p>{on() ? \"yes\" : \"no\"}</p>;\nconst i = <p>state: {on() ? \"yes\" : \"no\"}</p>;",
    ),
    ("text_run_empty", "const a = <p>{s() + \"\"}</p>;"),
    ("fragments", "const a = <>{a()}<b />text</>;\nconst b = <></>;\nconst c = <>{x}</>;"),
    ("document_order", "const a = <div>{a()}<p title={t()}>{b()}</p>{c()}{d()}<i />{e()}</div>;"),
    (
        "svg_and_math",
        "const a = <svg viewBox=\"0 0 1 1\"><circle r={r()} /><foreignObject><p>x</p></foreignObject></svg>;\nconst b = <g><path d=\"M0\" /></g>;\nconst c = <math><mi>x</mi></math>;",
    ),
    (
        "attributes",
        "const a = <a href={url()} title=\"t\" data-id={id} aria-label={label()} bool:hidden={h()} attr:x=\"1\" prop:foo={v} xlink:href=\"#i\" />;",
    ),
    (
        "merged_binds",
        "const a = <input value={v()} checked={c()} style={{ color: color() }} class={cls()} />;",
    ),
    ("class_sources", "const a = <i class={[\"a\", \"b\", { on: on() }]} />;"),
    (
        "static_class_and_style",
        "const a = <i class={[\"a\", { b: true, c: false }]} style={{ color: \"red\" }} />;",
    ),
    (
        "class_toggles",
        "const a = <p class={{ negative: n() < 0 }} />;\nconst b = <p class={[\"box\", { on: on(), big: false, wide: true }, [\"x\", { deep: d() }]]} />;\nconst c = <p class={[\"btn\", { active: active() }]} />;\nconst d = <p class={{ once: flag }} />;\nconst e = <p class={{ \"a b\": ab() }} />;\nconst f = <p class={[\"on\", { on: on() }]} />;\nconst g = <p class={[{ on: on(), off: false }, { on: other() }]} />;\nconst h = <p class={[\"x\", { x: false, y: y() }]} />;\nconst i = <p class={[cls(), { on: on() }]} />;",
    ),
    (
        "select_and_textarea",
        "const a = <select value={v()}>{options()}</select>;\nconst b = <textarea value=\"hi\" />;",
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
    ("native_spread", "const a = <div {...attrs} class=\"x\" ref={el}>{kids()}</div>;"),
    ("spread_children", "const a = <div {...attrs()} />;\nconst b = <p {...rest}><b /></p>;"),
    (
        "children_attribute",
        "const a = <div children={kids()} />;\nconst b = <div children={x()}><b /></div>;",
    ),
    (
        "props_simple",
        "function Greeting({ name, count }) {\n  return <p title={name}>{name}: {count}</p>;\n}",
    ),
    (
        "props_default",
        "const Button = ({ label = \"Save\", size = 2, disabled = false, icon = undefined }) => (\n  <button disabled={disabled} class={size}>{icon}{label}</button>\n);",
    ),
    (
        "props_default_rest",
        "function Link({ href = base + \"/\", label = href, ...rest }) {\n  \"use client\";\n  return <a href={href} {...rest}>{label}</a>;\n}",
    ),
    (
        "props_nested",
        "function User({ user: { name, \"first-name\": first = \"?\" }, 0: zero }) {\n  return <Card title={name} onClick={() => open(first)}>{zero}</Card>;\n}",
    ),
    (
        "props_rest_block",
        "function Link({ href, \"aria-label\": label, ...rest }) {\n  \"use client\";\n  return <a href={href} aria-label={label} {...rest} />;\n}",
    ),
    (
        "props_rest_expression",
        "const Card = ({ title, ...rest }) => <Panel {...rest} heading={title} />;",
    ),
    (
        "props_typescript",
        "type Props = { id: number; label: string; as: any; ref?: (el: Element) => void };\nexport function Row({ id, label, as: Tag, ref }: Props = { id: 0, label: \"\", as: \"li\" }) {\n  const data: typeof id = id;\n  const row = { id, label };\n  return <Tag ref={ref} onClick={() => select(row, data)}>{label}</Tag>;\n}",
    ),
    (
        "props_default_hoisted",
        "import { theme } from \"./theme\";\nconst Button = ({ label = theme.label, size, width = size, user: { name = theme.guest }, onPress = () => log(label, later), title = `${label}!`, style = { color: theme.color, [theme.key]: width }, items = [label, -size] as const, count = 0, later = label }: Props) => (\n  <button title={title} style={style} onClick={onPress}>{label}{width}{name}{items}{count}</button>\n);",
    ),
    (
        "props_default_refused",
        "function A({ a = new Date() }) { return <p>{a}</p>; }\nfunction B({ a = tag`x` }) { return <p>{a}</p>; }\nfunction C({ a = b, b }) { return <p>{a}{b}</p>; }\nfunction D({ a = x }) { const x = 1; return <p>{a}{x}</p>; }\nconst E = ({ a = <b /> }) => <p>{a}</p>;\nfunction F({ a = arguments[0] }) { return <p>{a}</p>; }",
    ),
    (
        "props_rejected",
        "function A({ [key]: a }) { return <p>{a}</p>; }\nfunction B({ a = f() }) { return <p>{a}</p>; }\nfunction C({ a: { b } = {} }) { return <p>{b}</p>; }\nfunction D({ a: { ...b } }) { return <p>{b}</p>; }\nfunction E({ a }) { a = 1; return <p>{a}</p>; }\nfunction F({ a }) { return <p>{arguments.length}{a}</p>; }\nfunction* G({ a }) { yield <p>{a}</p>; }\nconst H = function ({ a }, ref) { return <p ref={ref}>{a}</p>; };\nconst I = ({ a: [b] }) => <p>{b}</p>;",
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
        "conditionals",
        "const a = <div>{ok() ? <b>yes</b> : <i>no</i>}{open() && <p>{text()}</p>}</div>;\nconst b = <Card>{a() ? <b /> : null}{x()}</Card>;\nconst c = <p>{ok() ? \"yes\" : \"no\"}</p>;",
    ),
    (
        "warnings",
        "import { For, signal } from \"reze-js\";\nconst [n, setN] = signal(0); setN(1);\nfunction Greeting({ name = fallback() }) {\n  return <p key=\"k\" clas=\"x\" title={n} title=\"y\">{name}<For each={[1, 2]}>{(i) => i}</For></p>;\n}",
    ),
    (
        "return_position",
        "function A() { return <p title={t()}>x</p>; } const B = () => (<p>{n()}</p>);",
    ),
    (
        "show",
        "import { Show } from \"reze-js\";\nconst a = <div><Show when={n() >= 10}><p>big</p></Show></div>;\nconst b = <div>x<Show when={user()} fallback={<i>guest</i>}><>{user().name}<b/></></Show>y</div>;\nconst c = <Card><Show when={u()}>{(v) => <b>{v().name}</b>}</Show></Card>;",
    ),
    (
        "for_list",
        "import { For } from \"reze-js\";\nconst a = <ul><For each={rows()} fallback={<li>none</li>}>{(row, i) => <li>{i()}</li>}</For></ul>;\nconst b = <For each={rows()} key={(row) => row.id}>{(row) => <li>{row().name}</li>}</For>;",
    ),
    (
        "switch_match",
        "import { Match, Switch } from \"reze-js\";\nconst a = <div><Switch fallback={<i/>}><Match when={a()}><b/></Match><Match when={b()}>{(v) => <u>{v()}</u>}</Match></Switch></div>;",
    ),
    (
        "auto_selector",
        "import { For, computed, signal } from \"reze-js\";\nconst [selected, setSelected] = signal(0);\nconst [hovered, setHovered] = signal(0);\nconst active = computed(() => selected() + 1);\nexport const list = (\n  <For each={rows()}>\n    {(row, index) => {\n      const [local, setLocal] = signal(0);\n      return (\n        <tr class={selected() === row().id ? \"danger\" : \"\"} title={row().id !== selected() ? \"a\" : \"b\"} data-hover={hovered() === index()} data-active={active() === row().meta.id} onClick={() => setSelected(selected() === row().id ? 0 : row().id)}>\n          {selected() === row().id && <b>on</b>}\n          <td onInput={() => setLocal(1)} data-local={local() === row().id} data-other={selected() === other()} data-loose={selected() == row().id} />\n        </tr>\n      );\n    }}\n  </For>\n);\nexport const unrelated = <p>{selected() === 1}</p>;\nsetHovered(1);",
    ),
];

const HOT: &str = "import { signal } from \"reze-js\"; export function Counter() { const [n, setN] = signal(0); return <button onClick={() => setN(n() + 1)}>{n()}</button>; } export const Label = () => <b/>;";

const LINK_CASES: &[(&str, &str)] = &[
    ("link_static", "const a = <nav><a href=\"/x\" class=\"n\">X</a><a href={\"/y\"}>Y</a></nav>;"),
    ("link_root_quoted", "const a = <a href=\"/\">home</a>;"),
    (
        "link_dynamic",
        "const a = <a href={url()} title={t()}>go</a>;\nconst b = <a href={`/u/${id()}`} />;",
    ),
    ("link_constant_expression", "const to = \"/\" + page;\nconst a = <a href={to}>go</a>;"),
    (
        "link_aria_current_unclaimed",
        "const a = <a href=\"/x\" aria-current=\"page\" />;\nconst b = <a href={u()} aria-current={c()} />;",
    ),
    ("link_spread_unclaimed", "const a = <a {...p} href=\"/x\" />;"),
    (
        "link_external_unclaimed",
        "const a = <p><a href=\"#x\" /><a href=\"?q\" /><a href=\"mailto:a\" /><a href=\"https://x\" /><a href=\"//cdn\" /><a href=\"\" /><a /></p>;",
    ),
    ("link_hash_route", "const a = <a href=\"#/x\">X</a>;"),
    ("link_svg", "const a = <svg><a href=\"/x\"><text>x</text></a></svg>;"),
    ("link_header", "import \"./x.css\";\nconst a = <a href=\"/x\" />;"),
];

fn render(source: &str, options: &Options) -> String {
    let out = match compile(source, "case.tsx", options) {
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
    let options = Options { source_map: false, ..Options::default() };
    for (name, source) in CASES {
        insta::assert_snapshot!(*name, render(source, &options), source);
    }
}

#[test]
fn hot_snapshot() {
    let options = Options { source_map: false, debug_names: true, hot: true, links: None };
    insta::assert_snapshot!("hot", render(HOT, &options), HOT);
}

#[test]
fn link_snapshots() {
    let options =
        Options { source_map: false, links: Some("@rezejs/router".into()), ..Options::default() };
    for (name, source) in LINK_CASES {
        insta::assert_snapshot!(*name, render(source, &options), source);
    }
}
