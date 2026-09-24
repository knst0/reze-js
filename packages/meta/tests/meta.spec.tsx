import { renderToString } from "@rezejs/dom";
import { Show } from "@rezejs/dom/flow";
import { flushSync, signal } from "@rezejs/signals";
import { cleanup, mount } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

import { Base, Link, Meta, MetaProvider, renderTags, Style, Title, type HeadTag } from "../src";

afterEach(() => {
  cleanup();
  document.head.innerHTML = "";
});

test("head elements mount into the head and leave with their owner", () => {
  const { dispose } = mount(() => (
    <>
      <Meta name="description" content="about reze" />
      <Link rel="canonical" href="https://reze.dev/" />
      <Style>{"p { color: red }"}</Style>
      <p>body</p>
    </>
  ));
  expect(document.head.innerHTML).toBe(
    '<meta name="description" content="about reze"><link rel="canonical" href="https://reze.dev/"><style>p { color: red }</style>',
  );
  dispose();
  expect(document.head.innerHTML).toBe("");
});

test("the last title wins and the previous one returns when it goes", () => {
  const [isPageShown, setPageShown] = signal(true);
  const [name, setName] = signal("Ada");
  mount(() => (
    <>
      <Title>App</Title>
      <Show when={isPageShown()}>
        <Title>User {name()}</Title>
      </Show>
    </>
  ));
  expect(document.title).toBe("User Ada");
  expect(document.head.querySelectorAll("title")).toHaveLength(1);
  setName("Grace");
  flushSync();
  expect(document.title).toBe("User Grace");
  setPageShown(false);
  flushSync();
  expect(document.title).toBe("App");
  expect(document.head.querySelectorAll("title")).toHaveLength(1);
});

test("metas with the same name replace each other, attributes stay reactive", () => {
  const [content, setContent] = signal("page");
  mount(() => (
    <>
      <Meta name="description" content="site" />
      <Meta name="description" content={content()} />
      <Meta property="og:title" content="Reze" />
      <Base href="/a/" />
      <Base href="/b/" />
    </>
  ));
  expect(document.head.innerHTML).toBe(
    '<meta name="description" content="page"><meta property="og:title" content="Reze"><base href="/b/">',
  );
  setContent("changed");
  flushSync();
  expect(document.head.querySelector("meta")!.getAttribute("content")).toBe("changed");
});

test("server rendering collects tags for renderTags", () => {
  const tags: HeadTag[] = [];
  const html = renderToString(() => (
    <MetaProvider tags={tags}>
      <Title>App</Title>
      <Meta name="description" content={'a "quoted" <b>'} />
      <Title>{"Page <1>"}</Title>
      <Style>{"a > b { x: y } </style>"}</Style>
    </MetaProvider>
  ));
  expect(html).toBe("");
  expect(renderTags(tags)).toBe(
    '<meta name="description" content="a &quot;quoted&quot; <b>" data-rz-head>' +
      "<title data-rz-head>Page &lt;1></title>" +
      "<style data-rz-head>a > b { x: y } <\\/style></style>",
  );
});

test("a title comes before the page's static one", () => {
  document.head.innerHTML = "<title>Static</title>";
  const { dispose } = mount(() => <Title>Dynamic</Title>);
  expect(document.title).toBe("Dynamic");
  dispose();
  expect(document.title).toBe("Static");
});
