import type { RouteProps } from "@rezejs/router";

export default function Blog(props: RouteProps) {
  return (
    <section>
      <h1>Blog</h1>
      {props.children}
    </section>
  );
}
