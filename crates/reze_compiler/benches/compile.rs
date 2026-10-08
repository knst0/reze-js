extern crate codspeed_divan_compat as divan;
use reze_compiler::{Options, compile};
use std::fmt::Write as _;
use std::sync::LazyLock;

fn main() {
    divan::main();
}

const STATIC_TEMPLATE: &str = r##"const a = <div class="box"><p>hi</p><br /></div>;"##;

const TEXT_RUNS: &str = r##"const a = <p>doubled: {n() * 2}</p>;
const b = <p>{n() + 1}</p>;
const c = <p>{`${name()}`}</p>;
const d = <p>a {"<"} b &amp; {n() - 1} items</p>;
const e = <div>{n() * 2}<b />{x()}total: {n() % 3}<i />{(n() | 0) + 1}{-n()}</div>;
const f = <p>{label()}: {n() / 2}</p>;
const g = <p>size {SIZE * 2}</p>;
const h = <p>{on() ? "yes" : "no"}</p>;
const i = <p>state: {on() ? "yes" : "no"}</p>;"##;

const ATTRIBUTES: &str = r##"const a = <a href={url()} title="t" data-id={id} aria-label={label()} bool:hidden={h()} attr:x="1" prop:foo={v} xlink:href="#i" />;"##;

const EVENTS: &str = r##"const a = <div onClick={() => go()} onInput={[pick, 1]} onKeyDown={handler} on:scroll={s} onDoubleClick={d} onclick="legacy()" />;"##;

const COMPONENT_PROPS: &str = r##"const a = <Card title="t" count={n()} static={s} {...rest} onPick={() => pick()} ref={box.el}>body {n()}</Card>;"##;

const SHOW_COMPONENTS: &str = r##"import { Show } from "reze-js";
const a = <div><Show when={a()} fallback={<B />}><C /></Show></div>;"##;

const REPEAT: &str = r##"import { Repeat } from "reze-js";
import { signal } from "reze-js";
let size = signal(4);
let page = signal(1);
page = 2;
const a = <div><Repeat count={3}>{() => <i />}</Repeat></div>;
const b = <Repeat count={size} fallback={<p>none</p>}>{(index) => <Card n={index} onPick={() => pick(index)} />}</Repeat>;
const c = <Repeat count={page} fallback={<p>none</p>}>{(index) => <li>{index}</li>}</Repeat>;
const d = <Repeat count={rows()}>{() => <i />}</Repeat>;
const e = <Repeat count={0}>{() => <i />}</Repeat>;
const f = <Repeat count={2.5}>{() => <i />}</Repeat>;"##;

const ASYNC_COMPONENT_STEPS: &str = r##"export const Card = async ({ id }: { id: number }) => {
  const user = await fetchUser(id);
  const name = user.name.trim();
  const { Panel } = await import("./panel");
  const posts = await { then: (done) => done(load(user.id)) };
  return <Panel title={name}>{user.role}{posts.length}</Panel>;
};"##;

const COUNTER: &str = r##"import { computed, signal, Show } from "reze-js";
export function Counter(props) {
  let count = signal(0);
  const doubled = computed(count * 2);
  return (
    <section class="counter">
      <output class={{ negative: count < 0 }}>{count}</output>
      <p>doubled: {doubled}</p>
      <button onClick={() => count -= props.step}>minus</button>
      <button onClick={() => count = 0} disabled={count === 0}>reset</button>
      <button onClick={() => count += props.step}>plus</button>
      <Show when={count >= 10}><p class="note">a lot</p></Show>
    </section>
  );
}"##;

static LARGE_SOURCE: LazyLock<String> = LazyLock::new(|| {
    let mut source = String::from("const a = <div>");
    for i in 0..500 {
        let _ = write!(source, "<p class=\"row\" data-i=\"{i}\">row {i} {{n()}}</p>");
    }
    source.push_str("</div>;");
    source
});

fn bench_source(source: &str, bencher: divan::Bencher) {
    bencher.bench(|| {
        divan::black_box(compile(divan::black_box(source), "bench.tsx", &Options::default()))
    });
}

#[divan::bench]
fn static_template(bencher: divan::Bencher) {
    bench_source(STATIC_TEMPLATE, bencher);
}

#[divan::bench]
fn text_runs(bencher: divan::Bencher) {
    bench_source(TEXT_RUNS, bencher);
}

#[divan::bench]
fn attributes(bencher: divan::Bencher) {
    bench_source(ATTRIBUTES, bencher);
}

#[divan::bench]
fn events(bencher: divan::Bencher) {
    bench_source(EVENTS, bencher);
}

#[divan::bench]
fn component_props(bencher: divan::Bencher) {
    bench_source(COMPONENT_PROPS, bencher);
}

#[divan::bench]
fn show_components(bencher: divan::Bencher) {
    bench_source(SHOW_COMPONENTS, bencher);
}

#[divan::bench]
fn repeat(bencher: divan::Bencher) {
    bench_source(REPEAT, bencher);
}

#[divan::bench]
fn async_component_steps(bencher: divan::Bencher) {
    bench_source(ASYNC_COMPONENT_STEPS, bencher);
}

#[divan::bench]
fn large_tree(bencher: divan::Bencher) {
    bench_source(&LARGE_SOURCE, bencher);
}

#[divan::bench]
fn counter_signal(bencher: divan::Bencher) {
    bench_source(COUNTER, bencher);
}
