// Ported from @solidjs/router (MIT, Copyright (c) 2020-2022 Ryan Carniato).
import { delegateEvents } from "@rezejs/dom";
import { effect, onCleanup, untrack } from "@rezejs/signals";

import type { RouterState } from "./routing";
import { comparablePath } from "./utils";

export type FormSubmitHandler = (
  event: SubmitEvent,
  router: RouterState,
  actionBase: string,
) => void;

let formHandler: FormSubmitHandler | undefined;

/** Installed by the action module with its first action; the router's submit wiring holds no static action reference. */
export function setRouterFormHandler(handler: FormSubmitHandler): void {
  formHandler = handler;
}

export interface NativeEventOptions {
  preload?: boolean;
  explicitLinks?: boolean;
  actionBase?: string;
  transformUrl?: (url: string) => string;
}

/**
 * One document-level click/submit delegation per router: plain same-origin anchors navigate,
 * hover/focus preloads the target route, POST forms reach the action handler.
 */
export function setupNativeEvents(options: NativeEventOptions = {}): (router: RouterState) => void {
  const { preload = true, explicitLinks = false, actionBase = "/_server", transformUrl } = options;
  return (router) => {
    const navigate = router.navigatorFactory(router.base);
    let preloadTimeout = 0;
    let lastPreloaded: Element | null = null;
    const findAnchor = (
      event: Event,
    ): { anchor: HTMLAnchorElement | SVGAElement; url: URL } | undefined => {
      if (
        event.defaultPrevented ||
        (event instanceof MouseEvent &&
          (event.button !== 0 || event.metaKey || event.altKey || event.ctrlKey || event.shiftKey))
      ) {
        return undefined;
      }
      const anchor = event
        .composedPath()
        .find(
          (element): element is HTMLAnchorElement | SVGAElement =>
            element instanceof Node && (element as Element).nodeName.toUpperCase() === "A",
        );
      if (!anchor || (explicitLinks && !anchor.hasAttribute("link"))) return undefined;
      const isSvg = anchor.namespaceURI === "http://www.w3.org/2000/svg";
      const href = isSvg
        ? (anchor as SVGAElement).href.baseVal
        : (anchor as HTMLAnchorElement).href;
      const target = isSvg
        ? (anchor as SVGAElement).target.baseVal
        : (anchor as HTMLAnchorElement).target;
      if (target || (!href && !anchor.hasAttribute("state"))) return undefined;
      const rel = (anchor.getAttribute("rel") ?? "").split(/\s+/);
      if (anchor.hasAttribute("download") || rel.includes("external")) return undefined;
      const url = isSvg ? new URL(href, document.baseURI) : new URL(href);
      if (url.protocol !== "https:" && url.protocol !== "http:") return undefined;
      if (url.origin !== window.location.origin) return undefined;
      const basePath = untrack(router.base.path);
      if (basePath && !url.pathname.toLowerCase().startsWith(basePath.toLowerCase())) {
        return undefined;
      }
      return { anchor, url };
    };
    const handleAnchorClick = (event: Event): void => {
      const found = findAnchor(event);
      if (!found) return;
      const { anchor, url } = found;
      const to = router.parsePath(url.pathname + url.search + url.hash);
      const state = anchor.getAttribute("state");
      event.preventDefault();
      navigate(to, {
        resolve: false,
        replace: anchor.hasAttribute("replace"),
        scroll: !anchor.hasAttribute("noscroll"),
        state: state ? (JSON.parse(state) as unknown) : undefined,
      });
    };
    const preloadTarget = (url: URL, attribute: string | null): void => {
      if (transformUrl) url.pathname = transformUrl(url.pathname);
      router.preloadRoute(url, attribute !== "false");
    };
    const handleAnchorPreload = (event: Event): void => {
      const found = findAnchor(event);
      if (found) preloadTarget(found.url, found.anchor.getAttribute("preload"));
    };
    const handleAnchorMove = (event: Event): void => {
      clearTimeout(preloadTimeout);
      const found = findAnchor(event);
      if (!found) {
        lastPreloaded = null;
        return;
      }
      const { anchor, url } = found;
      if (lastPreloaded === anchor) return;
      lastPreloaded = anchor;
      preloadTimeout = window.setTimeout(
        () => preloadTarget(url, anchor.getAttribute("preload")),
        20,
      );
    };
    const handleFormSubmit = (event: Event): void => {
      if (formHandler) {
        formHandler(event as SubmitEvent, router, actionBase);
        return;
      }
      if (event.defaultPrevented) return;
      const form = event.target as HTMLFormElement;
      const submitter = (event as SubmitEvent).submitter as HTMLElement | null;
      const ref =
        (submitter?.hasAttribute("formaction") ? submitter.getAttribute("formaction") : null) ??
        form.getAttribute("action");
      if (!ref || form.method.toUpperCase() !== "POST") return;
      const url = new URL(ref, document.baseURI);
      if (!router.parsePath(url.pathname + url.search).startsWith(actionBase)) return;
      event.preventDefault();
      const data = new FormData(form, submitter);
      void import("./action").then((module) =>
        module.submitServerForm(
          router,
          router.parsePath(url.pathname + url.search),
          form,
          form.enctype === "multipart/form-data" ? data : new URLSearchParams(data as never),
        ),
      );
    };
    delegateEvents(["click", "submit"]);
    document.addEventListener("click", handleAnchorClick);
    if (preload) {
      document.addEventListener("mousemove", handleAnchorMove, { passive: true });
      document.addEventListener("focusin", handleAnchorPreload, { passive: true });
      document.addEventListener("touchstart", handleAnchorPreload, { passive: true });
    }
    document.addEventListener("submit", handleFormSubmit);
    onCleanup(() => {
      document.removeEventListener("click", handleAnchorClick);
      if (preload) {
        document.removeEventListener("mousemove", handleAnchorMove);
        document.removeEventListener("focusin", handleAnchorPreload);
        document.removeEventListener("touchstart", handleAnchorPreload);
      }
      document.removeEventListener("submit", handleFormSubmit);
    });
  };
}

/**
 * Gives every router-managed plain anchor the link vocabulary without a wrapper component:
 * `aria-current="page"` on exact matches, `data-active` on exact-or-prefix matches,
 * `data-pending` while the link is the in-flight target.
 */
export function setupLinkClaims(router: RouterState, explicitLinks?: boolean): void {
  const basePath = untrack(router.base.path);
  const exactByUs = new WeakMap<Element, true>();
  const managedPath = (anchor: HTMLAnchorElement | SVGAElement): string | undefined => {
    if (explicitLinks && !anchor.hasAttribute("link")) return undefined;
    const isSvg = anchor.namespaceURI === "http://www.w3.org/2000/svg";
    const href = isSvg ? (anchor as SVGAElement).href.baseVal : anchor.getAttribute("href");
    const target = isSvg
      ? (anchor as SVGAElement).target.baseVal
      : (anchor as HTMLAnchorElement).target;
    if (target || !href) return undefined;
    const rel = (anchor.getAttribute("rel") ?? "").split(/\s+/);
    if (anchor.hasAttribute("download") || rel.includes("external")) return undefined;
    let url: URL;
    try {
      url = new URL(href, document.baseURI);
    } catch {
      return undefined;
    }
    if (
      url.origin !== window.location.origin ||
      (basePath && !url.pathname.toLowerCase().startsWith(basePath.toLowerCase()))
    ) {
      return undefined;
    }
    return comparablePath(url.pathname);
  };
  const refresh = (anchor: HTMLAnchorElement): void => {
    untrack(() => {
      const location = decodeURI(comparablePath(router.location.pathname));
      const path = managedPath(anchor);
      const isRoot = path === "";
      const isActive =
        path !== undefined && (location === path || (!isRoot && location.startsWith(path + "/")));
      const isExact = path !== undefined && location === path;
      if (isActive) anchor.setAttribute("data-active", "");
      else anchor.removeAttribute("data-active");
      const pendingTarget = router.pendingTarget();
      if (
        router.isRouting() &&
        pendingTarget &&
        path !== undefined &&
        (decodeURI(comparablePath(pendingTarget.value)) === path ||
          (!isRoot && decodeURI(comparablePath(pendingTarget.value)).startsWith(path + "/")))
      ) {
        anchor.setAttribute("data-pending", "");
      } else {
        anchor.removeAttribute("data-pending");
      }
      if (isExact && !anchor.hasAttribute("aria-current")) {
        anchor.setAttribute("aria-current", "page");
        exactByUs.set(anchor, true);
      } else if (!isExact && exactByUs.get(anchor)) {
        anchor.removeAttribute("aria-current");
        exactByUs.delete(anchor);
      }
    });
  };
  const sweep = (root: ParentNode): void => {
    for (const anchor of root.querySelectorAll("a[href]")) refresh(anchor as HTMLAnchorElement);
  };
  effect(() => {
    void router.location.pathname;
    void router.isRouting();
    void router.pendingTarget();
    untrack(() => sweep(document));
  });
  const observer = new MutationObserver((mutations) => {
    for (const mutation of mutations) {
      if (mutation.type === "attributes" && mutation.target instanceof HTMLAnchorElement) {
        refresh(mutation.target);
      } else {
        for (const node of mutation.addedNodes) {
          if (node instanceof HTMLAnchorElement) refresh(node);
          else if (typeof (node as Element).querySelectorAll === "function") {
            sweep(node as unknown as ParentNode);
          }
        }
      }
    }
  });
  observer.observe(document.documentElement, {
    childList: true,
    subtree: true,
    attributes: true,
    attributeFilter: ["href"],
  });
  onCleanup(() => observer.disconnect());
}
