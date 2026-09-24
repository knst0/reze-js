import { mount } from "@rezejs/test-utils";
import { expect, test } from "vitest";

import { Link, Meta, Title } from "../src";

test("the client takes over equal server-rendered tags and drops the others", async () => {
  document.head.innerHTML =
    '<link rel="author" href="/humans.txt" data-rz-head>' +
    "<title data-rz-head>Old</title>" +
    '<meta name="robots" content="none" data-rz-head>';
  const serverLink = document.head.querySelector("link")!;
  mount(() => (
    <>
      <Link rel="author" href="/humans.txt" />
      <Title>New</Title>
      <Meta name="description" content="x" />
    </>
  ));
  const link = document.head.querySelector("link")!;
  expect(link).not.toBe(serverLink);
  expect(document.head.firstChild).toBe(link);
  await Promise.resolve();
  expect(document.head.innerHTML).toBe(
    '<link rel="author" href="/humans.txt"><title>New</title><meta name="description" content="x">',
  );
});
