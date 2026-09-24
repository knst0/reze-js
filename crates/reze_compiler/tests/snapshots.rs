use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use reze_compiler::{
    LinkOptions, ModuleInput, Options, SummaryOptions, Target, compile, link, summarize,
};

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
    ("document_order", "const a = <div>{a()}<p title={t()}>{b()}</p>{c()}{d()}<i />{e()}</div>;"),
    (
        "properties",
        "const a = <div><input value={v()} checked={c()} prop:x={x()} /><p textContent={t()} /><p innerHTML={h()} /><textarea value={v()} /><select value={v()}><option /></select></div>;",
    ),
    ("spread_children", "const a = <div {...attrs()} />;\nconst b = <p {...rest}><b /></p>;"),
    (
        "warnings",
        "import { signal } from \"reze-js\";\nconst [n, setN] = signal(0); setN(1);\nfunction Greeting({ name = fallback() }) {\n  return <p key=\"k\" clas=\"x\" title={n} title=\"y\">{name}<For each={[1, 2]}>{(i) => i}</For></p>;\n}",
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
        "props_async",
        "async function User({ id, ...rest }) {\n  const user = await fetchUser(id);\n  return <p {...rest}>{user.name}</p>;\n}",
    ),
    (
        "props_rejected",
        "function A({ [key]: a }) { return <p>{a}</p>; }\nfunction B({ a = f() }) { return <p>{a}</p>; }\nfunction C({ a: { b } = {} }) { return <p>{b}</p>; }\nfunction D({ a: { ...b } }) { return <p>{b}</p>; }\nfunction E({ a }) { a = 1; return <p>{a}</p>; }\nfunction F({ a }) { return <p>{arguments.length}{a}</p>; }\nfunction* G({ a }) { yield <p>{a}</p>; }\nconst H = function ({ a }, ref) { return <p ref={ref}>{a}</p>; };\nconst I = ({ a: [b] }) => <p>{b}</p>;",
    ),
    (
        "props_default_hoisted",
        "import { theme } from \"./theme\";\nconst Button = ({ label = theme.label, size, width = size, user: { name = theme.guest }, onPress = () => log(label, later), title = `${label}!`, style = { color: theme.color, [theme.key]: width }, items = [label, -size] as const, count = 0, later = label }: Props) => (\n  <button title={title} style={style} onClick={onPress}>{label}{width}{name}{items}{count}</button>\n);",
    ),
    (
        "props_default_rest",
        "function Link({ href = base + \"/\", label = href, ...rest }) {\n  \"use client\";\n  return <a href={href} {...rest}>{label}</a>;\n}",
    ),
    (
        "props_default_async",
        "async function User({ id = session.id, fallback = \"?\", ...rest }) {\n  const user = await fetchUser(id);\n  return <p {...rest}>{user.name ?? fallback}</p>;\n}",
    ),
    (
        "props_default_refused",
        "function A({ a = new Date() }) { return <p>{a}</p>; }\nfunction B({ a = tag`x` }) { return <p>{a}</p>; }\nfunction C({ a = b, b }) { return <p>{a}{b}</p>; }\nfunction D({ a = x }) { const x = 1; return <p>{a}{x}</p>; }\nconst E = ({ a = <b /> }) => <p>{a}</p>;\nfunction F({ a = arguments[0] }) { return <p>{a}</p>; }",
    ),
    (
        "computed_inlined",
        "import { computed, signal } from \"reze-js\";\nconst [name, setName] = signal(\"Reze\");\nconst greeting = computed(() => `Hi ${name()}`);\nexport const hello = <p onInput={() => setName(\"x\")}>{greeting()}</p>;\nfunction Counter() {\n  const [count, setCount] = signal(0);\n  const doubled = computed(() => count() * 2);\n  const label = computed(() => `n${count()}`);\n  const size = computed(() => (count() > 9 ? \"big\" : \"small\"));\n  const view = computed(() => <b>{count()}</b>);\n  return (\n    <div title={label()} onClick={() => setCount(count() + 1)}>\n      <Badge size={size()} />\n      {doubled()}\n      {view()}\n    </div>\n  );\n}",
    ),
    (
        "computed_kept",
        "import { computed, signal } from \"reze-js\";\nconst [n, setN] = signal(0);\nexport const total = computed(() => n() + 1);\nexport const view = <p onClick={() => setN(1)}>{total()}</p>;\nconst logged = computed(() => n() - 1);\nconsole.log(logged());\nfunction Panel() {\n  const twice = computed(() => n() * 2);\n  const typed: () => number = computed(() => n() + 2);\n  const nested = computed(() => n() + 3);\n  const shadowed = computed(() => n() + 4);\n  const row = <p title={typed()}>{twice()}{twice()}<For each={[1]}>{() => nested()}</For></p>;\n  if (row) {\n    const n = () => 0;\n    return <i>{shadowed()}{n()}</i>;\n  }\n  return row;\n}",
    ),
    (
        "store_unproxied",
        "import { store } from \"reze-js\";\nconst [todo, setTodo] = store({ title: \"\", done: false, meta: { count: 0, \"last-seen\": null } });\nconst [theme] = store({ color: \"red\" });\nexport const view = (\n  <p class={theme.color} onClick={() => setTodo((d) => { d.done = !d.done; d.meta.count *= 2; d.meta[\"last-seen\"] ??= Date.now(); d.meta.count++; })}>\n    {todo.title}{todo.meta.count}\n  </p>\n);\nexport const rename = (title) => setTodo((d) => d.title = title.trim());\nexport const reset = () => setTodo(function (d) { --d.meta.count; d.title = { text: \"\" }; });",
    ),
    (
        "store_refused",
        "import { store } from \"reze-js\";\nconst [whole] = store({ a: 1 });\nsave(whole);\nconst [nested] = store({ a: { b: 1 } });\nconst [keyed] = store({ a: 1 });\nconst [list, setList] = store({ items: [] });\nconst [later, setLater] = store({ a: 1 });\nsetLater((d) => { queue(() => { d.a = 2; }); });\nconst [valued, setValued] = store({ a: 1 });\nsetValued((d) => { log(d.a = 2); });\nexport const [exported] = store({ a: 1 });\nexport const view = (\n  <ul onClick={() => setList((d) => { d.items.push(1); })}>\n    {whole.a}{nested.a}{keyed[k]}{later.a}{valued.a}{exported.a}\n    {list.items.map((i) => <li>{i}</li>)}\n  </ul>\n);",
    ),
    (
        "store_arrays",
        "import { store } from \"reze-js\";\nconst [todos, setTodos] = store({ items: [{ done: false }], filter: \"all\" });\nconst [user, setUser] = store({ profile: { name: \"\", age: 0 } });\nexport const push = (x) => { todos.items.push(x); };\nexport const view = (\n  <ul onClick={() => setTodos((d) => { d.items.push({ done: true }); d.items[0].done = true; d.items[0].done ||= false; d.items.sort(); })}>\n    <For each={todos.items}>{(item) => <li>{item().done}</li>}</For>\n    {todos.items.length}{todos.items[0].done}{[...todos.items]}\n  </ul>\n);\nexport const fill = (name, age) => setUser((d) => { d.profile = { name, age }; });\nexport const bump = (i) => setUser((d) => { d.profile.age += i; });",
    ),
    (
        "store_arrays_refused",
        "import { store } from \"reze-js\";\nconst [bare, setBare] = store({ items: [1] });\nconst kept = bare.items;\nconst [compared, setCompared] = store({ items: [1] });\nconst same = compared.items === compared.items;\nconst [called, setCalled] = store({ items: [1] });\nuse(called.items);\nconst [spread, setSpread] = store({ items: [{ v: 1 }] });\nconst copy = { ...spread.items[0] };\nconst [deleted, setDeleted] = store({ items: [1] });\nsetDeleted((d) => { delete d.items[0]; });\nconst [shaped, setShaped] = store({ form: { a: 1, b: 2 } });\nsetShaped((d) => { d.form = other(); });\nconst [valued, setValued] = store({ items: [1] });\nsetValued((d) => { log(d.items.push(1)); });\nconst [jsx, setJsx] = store({ items: [0] });\nsetJsx((d) => { d.items[<b />] = 1; });\nexport const view = <ul>{kept.length}{same}{copy.v}</ul>;",
    ),
    (
        "auto_selector",
        "import { For, computed, signal } from \"reze-js\";\nconst [selected, setSelected] = signal(0);\nconst [hovered, setHovered] = signal(0);\nconst active = computed(() => selected() + 1);\nexport const list = (\n  <For each={rows()}>\n    {(row, index) => {\n      const [local, setLocal] = signal(0);\n      return (\n        <tr class={selected() === row().id ? \"danger\" : \"\"} title={row().id !== selected() ? \"a\" : \"b\"} data-hover={hovered() === index()} data-active={active() === row().meta.id} onClick={() => setSelected(selected() === row().id ? 0 : row().id)}>\n          {selected() === row().id && <b>on</b>}\n          <td onInput={() => setLocal(1)} data-local={local() === row().id} data-other={selected() === other()} data-loose={selected() == row().id} />\n        </tr>\n      );\n    }}\n  </For>\n);\nexport const unrelated = <p>{selected() === 1}</p>;\nsetHovered(1);",
    ),
    (
        "class_toggles",
        "const a = <p class={{ negative: n() < 0 }} />;\nconst b = <p class={[\"box\", { on: on(), big: false, wide: true }, [\"x\", { deep: d() }]]} />;\nconst c = <p class=\"btn\" classList={{ active: active() }} />;\nconst d = <p class={{ once: flag }} />;\nconst e = <p class={{ \"a b\": ab() }} />;\nconst f = <p class={[\"on\", { on: on() }]} />;\nconst g = <p class={{ on: on(), off: false }} classList={{ on: other() }} />;\nconst h = <p class={[\"x\", { x: false, y: y() }]} />;\nconst i = <p class={[cls(), { on: on() }]} />;",
    ),
    (
        "text_runs",
        "const a = <p>doubled: {n() * 2}</p>;\nconst b = <p>{n() + 1}</p>;\nconst c = <p>{`${name()}`}</p>;\nconst d = <p>a {\"<\"} b &amp; {n() - 1} items</p>;\nconst e = <div>{n() * 2}<b />{x()}total: {n() % 3}<i />{(n() | 0) + 1}{-n()}</div>;\nconst f = <p>{label()}: {n() / 2}</p>;\nconst g = <p>size {SIZE * 2}</p>;\nconst h = <p>{on() ? \"yes\" : \"no\"}</p>;\nconst i = <p>state: {on() ? \"yes\" : \"no\"}</p>;",
    ),
];

const TARGETS: &[(Target, &str)] =
    &[(Target::Client, ""), (Target::Server, "server__"), (Target::Hydrate, "hydrate__")];

fn render(source: &str, optimize: bool, target: Target) -> String {
    let options = Options { source_map: false, optimize, target, ..Options::default() };
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
    for (target, prefix) in TARGETS {
        for (name, source) in CASES {
            insta::assert_snapshot!(
                format!("{prefix}{name}"),
                render(source, true, *target),
                source
            );
        }
    }
}

#[test]
fn unoptimized_output_is_valid_too() {
    for (target, _) in TARGETS {
        for (_, source) in CASES {
            render(source, false, *target);
        }
    }
}

/// SPEC §15.2: a program of one entry module links to facts that change nothing in the output.
#[test]
fn a_single_entry_program_compiles_like_the_module_alone() {
    for (name, source) in CASES {
        let Ok(summary) = summarize(source, "case.tsx", &SummaryOptions::default()) else {
            continue;
        };
        let resolved = vec![None; summary.specifiers.len()];
        let module = ModuleInput { id: "case.tsx".into(), summary, resolved, is_entry: true };
        let linked =
            link(&[module], &LinkOptions { optimize: true, islands: true, root: String::new() });
        for (target, _) in TARGETS {
            let alone = Options { source_map: false, target: *target, ..Options::default() };
            let linked_options = Options {
                facts: Some(linked.facts["case.tsx"].clone()),
                ..Options { source_map: false, target: *target, ..Options::default() }
            };
            let code = |options: &Options| {
                compile(source, "case.tsx", options).ok().flatten().map(|out| out.code)
            };
            assert_eq!(code(&alone), code(&linked_options), "{name} [{target:?}]");
        }
    }
}
