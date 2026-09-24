import { effect, flush, signal } from "@rezejs/signals";
import { cleanup, mount } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

import { renderToString } from "../src";
import { Errored } from "../src/flow";
import { For } from "../src/list";

afterEach(cleanup);

function Boom(props: { message: string }): never {
  throw new Error(props.message);
}

test("an error while the children are created shows the fallback", () => {
  const { el } = mount(() => (
    <Errored fallback={(error) => <p>caught {(error() as Error).message}</p>}>
      <Boom message="early" />
    </Errored>
  ));
  expect(el.textContent).toBe("caught early");
});

test("an error from a later effect run or DOM binding reaches the boundary", () => {
  const [n, setN] = signal(0);
  const Effectful = () => {
    effect(() => {
      if (n() === 1) throw new Error("effect");
    });
    return <b>ok</b>;
  };
  const Binding = () => (
    <i
      title={(() => {
        if (n() === 2) throw new Error("binding");
        return "t";
      })()}
    />
  );
  const { el } = mount(() => (
    <>
      <Errored fallback={(error) => <p>{(error() as Error).message}</p>}>
        <Effectful />
      </Errored>
      <Errored fallback={(error) => <p>{(error() as Error).message}</p>}>
        <Binding />
      </Errored>
    </>
  ));
  expect(el.innerHTML).toBe('<b>ok</b><i title="t"></i>');
  setN(1);
  flush();
  expect(el.innerHTML).toBe('<p>effect</p><i title="t"></i>');
  setN(2);
  flush();
  expect(el.innerHTML).toBe("<p>effect</p><p>binding</p>");
});

test("a row added to a For inside the boundary that throws is caught", () => {
  const [rows, setRows] = signal(["a"]);
  const Row = (props: { name: string }) => {
    if (props.name === "bad") throw new Error("row");
    return <li>{props.name}</li>;
  };
  const { el } = mount(() => (
    <Errored fallback={<p>list failed</p>}>
      <ul>
        <For each={rows()}>{(row) => <Row name={row()} />}</For>
      </ul>
    </Errored>
  ));
  setRows(["a", "bad"]);
  flush();
  expect(el.innerHTML).toBe("<p>list failed</p>");
});

test("reset rebuilds the children", () => {
  let shouldThrow = true;
  const Flaky = () => {
    if (shouldThrow) throw new Error("flaky");
    return <b>fine</b>;
  };
  let retry!: () => void;
  const { el } = mount(() => (
    <Errored
      fallback={(_, reset) => {
        retry = reset;
        return <p>failed</p>;
      }}
    >
      <Flaky />
    </Errored>
  ));
  expect(el.textContent).toBe("failed");
  shouldThrow = false;
  retry();
  flush();
  expect(el.textContent).toBe("fine");
});

test("the inner boundary catches first; an error in a fallback reaches the outer one", () => {
  const { el } = mount(() => (
    <Errored fallback={<p>outer</p>}>
      <Errored fallback={<p>inner</p>}>
        <Boom message="x" />
      </Errored>
    </Errored>
  ));
  expect(el.textContent).toBe("inner");
  const second = mount(() => (
    <Errored fallback={<p>outer</p>}>
      <Errored fallback={() => <Boom message="fallback" />}>
        <Boom message="x" />
      </Errored>
    </Errored>
  ));
  expect(second.el.textContent).toBe("outer");
});

test("without a boundary the error is thrown", () => {
  expect(() => mount(() => <Boom message="loose" />)).toThrow("loose");
});

test("the server renders the fallback of a boundary whose children throw", () => {
  const html = renderToString(() => (
    <Errored fallback={<p>server fallback</p>}>
      <Boom message="server" />
    </Errored>
  ));
  expect(html).toContain("server fallback");
});
