import { signal as make } from "@rezejs/signals";
import { cleanup, mount, tick } from "@rezejs/testing-library";
import { signal } from "reze-js";
import { afterEach, expect, test } from "vite-plus/test";

import { ExternalCounter, setExtCount } from "./fixtures/exported-signals";

afterEach(cleanup);

test("static template interpolations render primitives with JavaScript coercion", () => {
  const { el } = mount(() => <p>{`a${1 + 2}:${"x" + 2}:${-0}:${true}:${null}:${undefined}z`}</p>);
  expect(el.innerHTML).toBe("<p>a3:x2:0:true:null:undefinedz</p>");
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

test("jsx text follows whitespace and entity rules", () => {
  const { el } = mount(() => <p title='x &amp; "y"'>a &amp; b c&nbsp;&#x41;&#66; {"<&>"}</p>);
  const p = el.firstChild as HTMLElement;
  expect(p.textContent).toBe("a & b c AB <&>");
  expect(p.getAttribute("title")).toBe('x & "y"');
});
