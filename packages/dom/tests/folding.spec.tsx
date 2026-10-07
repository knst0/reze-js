import { signal as make } from "@rezejs/signals";
import { cleanup, mount, tick } from "@rezejs/testing-library";
import { signal } from "reze-js";
import { afterEach, expect, test } from "vitest";

import { ExternalCounter, setExtCount } from "./fixtures/exported-signals";

afterEach(cleanup);

test("static template interpolations render primitives with JavaScript coercion", () => {
  const { el } = mount(() => <p>{`a${1 + 2}:${"x" + 2}:${-0}:${true}:${null}:${undefined}z`}</p>);
  expect(el.innerHTML).toBe("<p>a3:x2:0:true:null:undefinedz</p>");
});

test("numeric addition renders as a number, not string concatenation", () => {
  const { el } = mount(() => (
    <>
      <p>{1 + 2}</p>
      <p>
        {1 + 2 + "x"}:{"x" + 1 + 2}
      </p>
    </>
  ));
  expect(el.innerHTML).toBe("<p>3</p><p>3x:x12</p>");
});

test("negative zero, NaN and infinities follow numeric truthiness", () => {
  const { el } = mount(() => (
    <p>
      {-0 ? "wrong" : "zero"}
      {0 / 0 ? "wrong" : "nan"}
      {1 / 0 ? "infinity" : "wrong"}
    </p>
  ));
  expect(el.innerHTML).toBe("<p>zeronaninfinity</p>");
});

test("global nonfinite numbers fold their branches but render at runtime", () => {
  const branches = mount(() => (
    <p>
      {NaN ? "wrong" : "nan"}
      {-Infinity ? "infinity" : "wrong"}
    </p>
  ));
  expect(branches.el.innerHTML).toBe("<p>naninfinity</p>");
  const texts = mount(() => <p>{`${NaN}:${Infinity}:${1.5}:${1e20}`}</p>);
  expect(texts.el.innerHTML).toBe("<p>NaN:Infinity:1.5:100000000000000000000</p>");
});

function pickBranch(undefined: string | undefined) {
  return <p>{undefined ? "yes" : "no"}</p>;
}

function pickNaN(NaN: number) {
  return <p>{NaN ? "on" : "off"}</p>;
}

function pickInfinity(Infinity: number) {
  return <p>{Infinity ? "up" : "down"}</p>;
}

function pairShaped(undefined: string, value: () => string) {
  return <p>{`a${undefined}:${value()}`}</p>;
}

function Show(props: { when: unknown; children?: unknown }) {
  return <b>{props.when ? "yes" : "no"}</b>;
}

test("shadowed globals keep their branches", () => {
  const { el } = mount(() => (
    <>
      {pickBranch("s")}
      {pickBranch(undefined)}
      {pickNaN(0)}
      {pickInfinity(2)}
    </>
  ));
  expect(el.innerHTML).toBe("<p>yes</p><p>no</p><p>off</p><p>up</p>");
});

test("shadowed template holes stay live", () => {
  const [value, setValue] = make("x");
  const { el } = mount(() => pairShaped("s", value));
  expect(el.innerHTML).toBe("<p>as:x</p>");
  setValue("y");
  tick();
  expect(el.innerHTML).toBe("<p>as:y</p>");
});

test("a component shadowing an intrinsic name renders as a component", () => {
  const { el } = mount(() => (
    <>
      <Show when={1}>
        <i />
      </Show>
      <Show when={0}>
        <i />
      </Show>
    </>
  ));
  expect(el.innerHTML).toBe("<b>yes</b><b>no</b>");
});

test("folded signals render their constants with correct truthiness", () => {
  const [zero] = make(0);
  const [text] = make("0");
  const { el } = mount(() => (
    <p>
      {zero() ? "wrong" : "zero"}
      {text() ? "text" : "wrong"}
    </p>
  ));
  expect(el.innerHTML).toBe("<p>zerotext</p>");
});

test("falsy logical-and renders its left value", () => {
  const { el } = mount(() => {
    let zero = signal(0);
    return (
      <p>
        {1 - 1 && <b />}
        {-0 && <b />}
        {zero && <b />}
      </p>
    );
  });
  expect(el.innerHTML).toBe("<p>000</p>");
});

test("a nested function shadowing the signal factory stays a plain call", () => {
  const [a] = make(1);
  function nested(make: (n: number) => number): number {
    return make(2);
  }
  const { el } = mount(() => (
    <p>
      {a()}:{nested((n) => n * 10)}
    </p>
  ));
  expect(el.innerHTML).toBe("<p>1:20</p>");
});

test("nested templates and escapes fold to rendered text", () => {
  const { el } = mount(() => <p>{`a\u0062${`c${2 - 1}`}\x64`}</p>);
  expect(el.innerHTML).toBe("<p>abc1d</p>");
});

test("jsx text follows whitespace and entity rules", () => {
  const { el } = mount(() => <p title='x &amp; "y"'>a &amp; b c&nbsp;&#x41;&#66; {"<&>"}</p>);
  const p = el.firstChild as HTMLElement;
  expect(p.textContent).toBe("a & b c AB <&>");
  expect(p.getAttribute("title")).toBe('x & "y"');
});

test("exported signals stay live for external writes", () => {
  setExtCount(5);
  const { el } = mount(() => <ExternalCounter />);
  expect(el.innerHTML).toBe("<p>5</p>");
  setExtCount(6);
  tick();
  expect(el.innerHTML).toBe("<p>6</p>");
});

test("locals colliding with runtime helper names keep working", () => {
  const _$template = (s: string): string => `user:${s}`;
  function _$insert(a: string, b: string): string {
    return `${a}+${b}`;
  }
  const [n, setN] = make(1);
  const { el } = mount(() => <p title={_$template("t")}>{_$insert("v", String(n()))}</p>);
  expect(el.innerHTML).toBe('<p title="user:t">v+1</p>');
  setN(2);
  tick();
  expect(el.innerHTML).toBe('<p title="user:t">v+2</p>');
});
