type Ref<E> = E | ((el: E) => void);

type Loose = any;

interface DOMAttributes<E> {
  children?: JSX.Element;
  ref?: Ref<E>;
  [attribute: string]: Loose;
}

type Intrinsic<M> = { [K in keyof M]: DOMAttributes<M[K]> };

export namespace JSX {
  export type Element = Node | ArrayElement | FunctionElement | (string & {}) | number | bigint | boolean | null | undefined;
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
