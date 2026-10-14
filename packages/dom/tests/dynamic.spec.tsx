import { signal } from "@rezejs/signals";
import { cleanup, fire, mount, tick } from "@rezejs/testing-library";
import { dynamic, dynamicElement, onCleanup } from "reze-js";
import { afterEach, expect, test } from "vite-plus/test";

afterEach(cleanup);

test("dynamicElement renders a tag name chosen at runtime with its props, events and children, in the namespace of the tag", () => {
  const clicks: string[] = [];
  const [tag, setTag] = signal("input");
  const Field = dynamicElement(() => tag());
  const { el } = mount(() => (
    <Field value="typed" onClick={() => clicks.push("clicked")}>
      <span>inside</span>
    </Field>
  ));
  const input = el.firstChild as HTMLInputElement;
  expect(input.value).toBe("typed");
  fire(input, "click");
  expect(clicks).toEqual(["clicked"]);

  for (const [name, namespace, valueAttribute] of [
    ["circle", "http://www.w3.org/2000/svg", "typed"],
    ["svg", "http://www.w3.org/2000/svg", "typed"],
    ["math", "http://www.w3.org/1998/Math/MathML", "typed"],
    ["a", "http://www.w3.org/1999/xhtml", null],
  ]) {
    setTag(name!);
    tick();
    const node = el.firstChild as Element;
    expect([node.localName, node.namespaceURI, node.getAttribute("value")]).toEqual([name, namespace, valueAttribute]);
  }
  expect((el.firstChild as Element).innerHTML).toBe("<span>inside</span>");

  setTag("");
  tick();
  expect(el.innerHTML).toBe("");
});

test("tag name literals of a dynamic source render in their namespace, and returning the same tag keeps its element", () => {
  const [isRound, setRound] = signal(true);
  const [unrelated, setUnrelated] = signal(0);
  const Shape = dynamic(() => (unrelated(), isRound() ? "circle" : "button"));
  const { el } = mount(() => <Shape title="shape" />);
  const circle = el.firstChild as Element;
  expect([circle.localName, circle.namespaceURI, circle.getAttribute("title")]).toEqual(["circle", "http://www.w3.org/2000/svg", "shape"]);

  setUnrelated(1);
  tick();
  expect(el.firstChild).toBe(circle);

  setRound(false);
  tick();
  expect(el.innerHTML).toBe('<button title="shape"></button>');
});

test("dynamic given a tag name at runtime reports that dynamicElement is needed", () => {
  const [tag] = signal("div");
  const Picked = dynamic(() => tag());
  expect(() => mount(() => <Picked />)).toThrow(
    '[reze] dynamic: the tag name "div" is not a string literal its source returns; use dynamicElement for tag names chosen at runtime',
  );
});

test("nested functions inside a dynamic source are not tag sources", () => {
  const calls: string[] = [];
  const Picked = dynamic(() => {
    const inner = (): string => "span";
    calls.push(inner());
    return "section";
  });
  const { el } = mount(() => <Picked>hi</Picked>);
  expect(el.innerHTML).toBe("<section>hi</section>");
  expect(calls).toEqual(["span"]);
});
