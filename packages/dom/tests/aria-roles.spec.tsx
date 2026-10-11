import { cleanup, mount } from "@rezejs/testing-library";
import { afterEach, expect, test } from "vite-plus/test";

afterEach(cleanup);

test("mdn aria roles render the role attribute", () => {
  const { el } = mount(() => (
    <>
      <div role="comment" />
      <div role="generic" />
      <div role="mark" />
      <div role="suggestion" />
    </>
  ));
  const roles = [...el.querySelectorAll(":scope > div")].map((div) => div.getAttribute("role"));
  expect(roles).toEqual(["comment", "generic", "mark", "suggestion"]);
});
