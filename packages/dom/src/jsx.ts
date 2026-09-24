// JSX types for `jsxImportSource: "@rezejs/dom"`. Attribute values are loose; element types and
// `ref` are precise so refs receive the right element.

type Ref<E> = E | ((el: E) => void);

// oxlint-disable-next-line typescript/no-explicit-any
type Loose = any;

interface DOMAttributes<E> {
  children?: JSX.Element;
  ref?: Ref<E>;
  [attribute: string]: Loose;
}

type Intrinsic<M> = { [K in keyof M]: DOMAttributes<M[K]> };

export namespace JSX {
  // `Promise<unknown>`: async components. The compiler rewrites them into sync components
  // with a `trackAsync` subscription, so by runtime they return `Element`; the source-level
  // `Promise` must stay a valid component return or `<AsyncComp />` fails with TS2786.
  // `unknown` (not `Element`) keeps the union non-recursive: `Promise<JSX.Element>` as an
  // annotation would otherwise trip TS1062 on itself.
  export type Element =
    | Node
    | ArrayElement
    | FunctionElement
    | Promise<unknown>
    | (string & {})
    | number
    | bigint
    | boolean
    | null
    | undefined;
  export interface ArrayElement extends Array<Element> {}
  export interface FunctionElement {
    (): Element;
  }
  export interface ElementChildrenAttribute {
    children: {};
  }
  export interface IntrinsicAttributes {}
  export interface IntrinsicElements
    extends
      Intrinsic<HTMLElementTagNameMap>,
      Intrinsic<Omit<SVGElementTagNameMap, keyof HTMLElementTagNameMap>>,
      Intrinsic<Omit<MathMLElementTagNameMap, keyof HTMLElementTagNameMap>> {
    [tag: string]: DOMAttributes<Loose>;
  }
}
