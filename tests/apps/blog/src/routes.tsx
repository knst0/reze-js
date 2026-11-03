import { Show, useLinkState, useLocation, useSearchParams } from "reze-js";

function Where() {
  const location = useLocation();
  const search = useSearchParams();
  const home = useLinkState("/");
  return (
    <p class="where" data-home={home.active}>
      At {location.pathname}
      {location.search}
      <Show when={search.get("ref")}>{(ref) => <span> from {ref}</span>}</Show>
    </p>
  );
}

export default function Root(props: { children: unknown }) {
  return (
    <div>
      <head>
        <title>Blog</title>
      </head>
      <nav>
        <a href="/">Home</a>
      </nav>
      <Where />
      {props.children}
    </div>
  );
}
