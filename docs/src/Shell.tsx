import { useLinkState, useLocation, type Href } from "@rezejs/router";
import { computed, For, Show, signal, type JSX } from "reze-js";
import { paths } from "virtual:reze-routes";

import { useFluidHover } from "./hooks/useFluidHover";
import { pages, sections, type NavPage } from "./navigation";

export function Shell(props: { children: JSX.Element }) {
  let menuOpen = signal(false);
  return (
    <>
      <a
        class="docs-skip"
        href="#docs-content"
        onClick={(event: MouseEvent) => {
          event.preventDefault();
          document.getElementById("docs-content")?.focus();
        }}
      >
        Skip to content
      </a>
      <div class="docs-layout">
        <aside class="docs-sidebar">
          <div class="docs-sidebar-header">
            <a class="docs-brand" href={paths.installation()}>
              Reze<span>docs</span>
            </a>
            <button
              class="docs-menu-toggle"
              type="button"
              aria-expanded={menuOpen ? "true" : "false"}
              aria-controls="docs-navigation"
              onClick={() => (menuOpen = !menuOpen)}
            >
              {menuOpen ? "Close" : "Menu"}
            </button>
          </div>
          <nav
            id="docs-navigation"
            class="docs-navigation"
            aria-label="Documentation"
            data-open={menuOpen ? "" : null}
            onClick={(event) => {
              if ((event.target as HTMLElement).closest("a")) menuOpen = false;
            }}
          >
            <For each={sections}>
              {(section) => (
                <>
                  <Show when={section.title}>{(title) => <h2>{title}</h2>}</Show>
                  <NavGroup>
                    <For each={section.pages}>{(page) => <NavLink href={page.href}>{page.label}</NavLink>}</For>
                  </NavGroup>
                </>
              )}
            </For>
          </nav>
        </aside>
        <main id="docs-content" class="docs-content" tabindex={-1}>
          {props.children}
          <Pager />
        </main>
      </div>
    </>
  );
}

function Pager() {
  const location = useLocation();
  const index = computed(pageIndex(location().pathname));
  const previous = computed(index > 0 ? pages[index - 1] : undefined);
  const next = computed(index !== -1 ? pages[index + 1] : undefined);
  return (
    <Show when={index !== -1}>
      <nav class="docs-pager" aria-label="Pagination">
        <Show when={previous}>{(page) => <PagerLink page={page} rel="prev" />}</Show>
        <Show when={next}>{(page) => <PagerLink page={page} rel="next" />}</Show>
      </nav>
    </Show>
  );
}

function PagerLink(props: { page: NavPage; rel: "prev" | "next" }) {
  return (
    <a href={props.page.href} rel={props.rel} data-rel={props.rel}>
      <span>{props.rel === "prev" ? "Previous" : "Next"}</span>
      {props.page.label}
    </a>
  );
}

function pageIndex(pathname: string) {
  const normalized = pathname.length > 1 ? pathname.replace(/\/$/, "") : pathname;
  return pages.findIndex((page) => page.href === normalized);
}

function NavGroup(props: { children: JSX.Element }) {
  const fluidHover = useFluidHover();
  return (
    <div class="docs-nav-group" ref={(element: HTMLDivElement) => fluidHover(element)}>
      <span class="docs-hover-highlight" data-fluid-hover-highlight="" aria-hidden="true" />
      {props.children}
    </div>
  );
}

function NavLink(props: { href: Href; children: JSX.Element }) {
  const state = useLinkState(() => props.href);
  return (
    <a href={props.href} aria-current={state.current() ? "page" : undefined}>
      {props.children}
    </a>
  );
}
