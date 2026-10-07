import { useLinkState, type Href } from "@rezejs/router";
import { $signal, type JSX } from "reze-js";
import { paths } from "virtual:reze-routes";

import { useFluidHover } from "./hooks/useFluidHover";

export function Shell(props: { children: JSX.Element }) {
  let menuOpen = $signal(false);
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
            <button
              class="docs-menu-toggle"
              type="button"
              aria-expanded={menuOpen ? "true" : "false"}
              aria-controls="docs-navigation"
              onClick={() => (menuOpen = !menuOpen)}
            >
              {menuOpen ? "Close menu" : "Menu"}
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
            <NavGroup>
              <NavLink href={paths.installation()}>Getting started</NavLink>
            </NavGroup>
            <h2>Core concepts</h2>
            <NavGroup>
              <NavLink href={paths.components()}>Components and JSX</NavLink>
              <NavLink href={paths.reactivity()}>Reactivity</NavLink>
              <NavLink href={paths.lifecycle()}>Lifecycle and cleanup</NavLink>
              <NavLink href={paths["familiar-patterns"]()}>Familiar patterns</NavLink>
            </NavGroup>
            <h2>Building applications</h2>
            <NavGroup>
              <NavLink href={paths["building-interfaces"]()}>Building interfaces</NavLink>
              <NavLink href={paths.routing()}>Routing</NavLink>
            </NavGroup>
            <h2>Reference</h2>
            <NavGroup>
              <NavLink href={paths["signals-api"]()}>Signals API</NavLink>
              <NavLink href={paths["router-api"]()}>Router API</NavLink>
              <NavLink href={paths["vite-plugin"]()}>Vite plugin</NavLink>
              <NavLink href={paths.compiler()}>Compiler</NavLink>
            </NavGroup>
          </nav>
        </aside>
        <main id="docs-content" class="docs-content" tabindex={-1}>
          {props.children}
        </main>
      </div>
    </>
  );
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

function NavLink(props: { href: JSX.IntrinsicElements["a"]["href"]; children: JSX.Element }) {
  const state = useLinkState(() => props.href as Href);
  return (
    <a href={props.href} aria-current={state.current() ? "page" : undefined}>
      {props.children}
    </a>
  );
}
