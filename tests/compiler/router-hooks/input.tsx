import { buildPaths, Show, signal, useLinkState, useLocation, useSearchParams } from "reze-js";

function NavLink(props: { href: string; label: string }) {
  const link = useLinkState(props.href);
  return (
    <a href={props.href} aria-current={link.active ? "page" : undefined} data-active={link.active}>
      {props.label}
    </a>
  );
}

export default function Search() {
  const location = useLocation();
  const params = useSearchParams();
  let copied = signal("");
  return (
    <main>
      <NavLink href="/" label="Home" />
      <NavLink href="/about" label="About" />
      <a href={buildPaths("/docs/*slug", { slug: "guide/a b" })}>Guide</a>
      <a href={buildPaths("/blog/:page?", { page: params.get("page") ?? undefined })}>Blog</a>
      <p>
        {location.pathname}
        {location.search}
      </p>
      <p>Query: {params.get("q")}</p>
      <Show when={params.has("page")}>
        <p>Paged</p>
      </Show>
      <button onClick={() => (copied = location.pathname + params.get("q"))}>Copy {copied}</button>
    </main>
  );
}
