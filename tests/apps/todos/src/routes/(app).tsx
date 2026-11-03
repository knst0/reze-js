import { guardRoute, loader, redirect } from "reze-js/server";

export const guard = guardRoute(({ context }) => {
  if (!context.user) redirect("/login");
});

export const load = loader(async ({ context, response }) => {
  response.headers.set("Cache-Control", "public, max-age=600");
  return { user: context.user! };
});

export default function App(props: { data: { user: string }; children: unknown }) {
  return (
    <section>
      <p>Signed in as {props.data.user}</p>
      {props.children}
    </section>
  );
}
