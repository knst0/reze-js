import {
  createComponent,
  createContext,
  isServerRender,
  spread,
  ssrChild,
  ssrSpread,
  useContext,
} from "@rezejs/dom";
import type { JSX } from "@rezejs/dom/jsx-runtime";
import { onCleanup, untrack } from "@rezejs/signals";

/** A head element rendered on the server, collected by `<MetaProvider tags>`. */
export interface HeadTag {
  /** Tags sharing a key replace each other: only the last one rendered is kept. */
  key: string | undefined;
  html: string;
}

export type TitleProps = { children?: JSX.Element };
export type MetaProps = JSX.IntrinsicElements["meta"];
export type LinkProps = JSX.IntrinsicElements["link"];
export type StyleProps = JSX.IntrinsicElements["style"];
export type BaseProps = JSX.IntrinsicElements["base"];

type TagProps = Record<string, unknown>;

const ServerTagMarker = "data-rz-head";
const MetaContext = createContext<HeadTag[] | undefined>(undefined);

/**
 * Collects the head elements rendered inside it into `tags` during server rendering; write
 * `renderTags(tags)` into the page's `<head>`. In the browser it only renders its children:
 * head elements work without it there.
 */
export function MetaProvider(props: { tags?: HeadTag[]; children?: JSX.Element }): JSX.Element {
  return createComponent(MetaContext, {
    value: props.tags,
    get children() {
      return props.children;
    },
  });
}

/** The HTML of `tags` for the page's `<head>`, keeping the last tag of each key. */
export function renderTags(tags: HeadTag[]): string {
  const lastIndexByKey = new Map<string, number>();
  tags.forEach((tag, index) => {
    if (tag.key !== undefined) lastIndexByKey.set(tag.key, index);
  });
  return tags
    .filter((tag, index) => tag.key === undefined || lastIndexByKey.get(tag.key) === index)
    .map((tag) => tag.html)
    .join("");
}

/** `document.title` while it is the last `<Title>` rendered; the previous one returns when it goes. */
export function Title(props: TitleProps): JSX.Element {
  return headTag("title", props, "title");
}

/**
 * A `<meta>` in the head. Metas with the same `name`, `property`, `http-equiv`, `itemprop` or
 * `charset` replace each other: the last one rendered wins until it is removed.
 */
export function Meta(props: MetaProps): JSX.Element {
  return headTag(
    "meta",
    props,
    untrack(() => metaKey(props)),
  );
}

/** A `<link>` in the head, e.g. a stylesheet or a canonical URL. */
export function Link(props: LinkProps): JSX.Element {
  return headTag("link", props, undefined);
}

/** A `<style>` in the head; its children are the CSS text. */
export function Style(props: StyleProps): JSX.Element {
  return headTag("style", props, undefined);
}

/** The document `<base>`; the last one rendered wins. */
export function Base(props: BaseProps): JSX.Element {
  return headTag("base", props, "base");
}

const MetaKeyAttributes = ["name", "property", "http-equiv", "itemprop", "charset"];

function metaKey(props: TagProps): string | undefined {
  for (const attribute of MetaKeyAttributes) {
    const value = props[attribute];
    if (typeof value === "string")
      return attribute === "charset" ? "charset" : `meta:${attribute}:${value}`;
  }
  return undefined;
}

function headTag(tag: string, props: TagProps, key: string | undefined): JSX.Element {
  if (isServerRender()) {
    useContext(MetaContext)?.push({ key, html: serverTag(tag, props) });
    return undefined;
  }
  const element = document.createElement(tag);
  spread(element, props);
  mount(key, element);
  onCleanup(() => unmount(key, element));
  return undefined;
}

const VoidTags = new Set(["meta", "link", "base"]);

function serverTag(tag: string, props: TagProps): string {
  const open = `<${tag}${ssrSpread(props)} ${ServerTagMarker}>`;
  if (VoidTags.has(tag)) return open;
  const content = tag === "style" ? styleText(props.children) : ssrChild(props.children);
  return `${open}${content}</${tag}>`;
}

function styleText(children: unknown): string {
  const text = typeof children === "string" || typeof children === "number" ? `${children}` : "";
  return text.replaceAll("</", "<\\/");
}

const stacksByKey = new Map<string, Element[]>();
let serverTags: Element[] | undefined;

/**
 * The server-rendered head element equal to `element`, taken so `element` replaces it in place.
 * Server elements no client element claimed by the end of the current task are removed.
 */
function claimServerTag(element: Element): Element | undefined {
  if (serverTags === undefined) {
    const found = [...document.head.querySelectorAll(`[${ServerTagMarker}]`)];
    for (const tag of found) tag.removeAttribute(ServerTagMarker);
    serverTags = found;
    queueMicrotask(() => {
      for (const tag of found.splice(0)) tag.remove();
    });
  }
  const index = serverTags.findIndex((tag) => tag.isEqualNode(element));
  return index < 0 ? undefined : serverTags.splice(index, 1)[0];
}

/** A `<title>` goes before the page's static one: `document.title` reads the first. */
function attach(element: Element): void {
  const serverTag = claimServerTag(element);
  if (serverTag !== undefined) serverTag.replaceWith(element);
  else if (element.localName === "title") {
    document.head.insertBefore(element, document.head.querySelector("title"));
  } else document.head.append(element);
}

function mount(key: string | undefined, element: Element): void {
  if (key === undefined) {
    attach(element);
    return;
  }
  let stack = stacksByKey.get(key);
  if (stack === undefined) stacksByKey.set(key, (stack = []));
  stack[stack.length - 1]?.remove();
  stack.push(element);
  attach(element);
}

function unmount(key: string | undefined, element: Element): void {
  element.remove();
  const stack = key === undefined ? undefined : stacksByKey.get(key);
  if (stack === undefined) return;
  const index = stack.indexOf(element);
  const wasShown = index === stack.length - 1;
  stack.splice(index, 1);
  const previous = stack[stack.length - 1];
  if (previous === undefined) stacksByKey.delete(key!);
  else if (wasShown) attach(previous);
}
