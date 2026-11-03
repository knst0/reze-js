/** The DOM event behind each `on*` handler name, as the compiler lowers it. */
interface HandlerEvents {
  Abort: "abort";
  AnimationEnd: "animationend";
  AnimationStart: "animationstart";
  AuxClick: "auxclick";
  BeforeInput: "beforeinput";
  Blur: "blur";
  Cancel: "cancel";
  Change: "change";
  Click: "click";
  Close: "close";
  ContextMenu: "contextmenu";
  Copy: "copy";
  Cut: "cut";
  DoubleClick: "dblclick";
  Drag: "drag";
  DragEnd: "dragend";
  DragEnter: "dragenter";
  DragLeave: "dragleave";
  DragOver: "dragover";
  DragStart: "dragstart";
  Drop: "drop";
  Error: "error";
  Focus: "focus";
  FocusIn: "focusin";
  FocusOut: "focusout";
  Input: "input";
  Invalid: "invalid";
  KeyDown: "keydown";
  KeyUp: "keyup";
  Load: "load";
  MouseDown: "mousedown";
  MouseEnter: "mouseenter";
  MouseLeave: "mouseleave";
  MouseMove: "mousemove";
  MouseOut: "mouseout";
  MouseOver: "mouseover";
  MouseUp: "mouseup";
  Paste: "paste";
  Pause: "pause";
  Play: "play";
  PointerCancel: "pointercancel";
  PointerDown: "pointerdown";
  PointerEnter: "pointerenter";
  PointerLeave: "pointerleave";
  PointerMove: "pointermove";
  PointerUp: "pointerup";
  Reset: "reset";
  Scroll: "scroll";
  ScrollEnd: "scrollend";
  Select: "select";
  Submit: "submit";
  Toggle: "toggle";
  TouchEnd: "touchend";
  TouchMove: "touchmove";
  TouchStart: "touchstart";
  TransitionEnd: "transitionend";
  Wheel: "wheel";
}

type Handlers<E extends Element> = {
  [K in keyof HandlerEvents as `on${K}`]?: (event: HTMLElementEventMap[HandlerEvents[K]] & { readonly currentTarget: E }) => void;
};

type Attributes<E extends Element> = Handlers<E> & {
  /** Receives the element in the browser; its chunk loads when the element enters the viewport. */
  ref?: (element: E) => void;
  /** Loads the chunk of this element's handlers before the first interaction. */
  eager?: boolean;
  children?: unknown;
  [attribute: string]: unknown;
};

type HtmlElements = { [K in keyof HTMLElementTagNameMap]: Attributes<HTMLElementTagNameMap[K]> };
type SvgElements = {
  [K in Exclude<keyof SVGElementTagNameMap, keyof HTMLElementTagNameMap>]: Attributes<SVGElementTagNameMap[K]>;
};

export declare namespace JSX {
  type Element = string | number | boolean | null | undefined | { readonly __markup: true };
  interface ElementChildrenAttribute {
    children: {};
  }
  interface IntrinsicAttributes {
    key?: unknown;
  }
  interface IntrinsicElements extends HtmlElements, SvgElements {
    [tag: string]: Record<string, unknown>;
  }
}
