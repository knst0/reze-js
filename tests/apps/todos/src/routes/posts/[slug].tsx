import { loader, type LoaderArgs } from "reze-js/server";

import { render } from "../../db";

export const prerender = true;
export const revalidate = 60;
export const entries = () => [{ slug: "first" }, { slug: "second" }];

export const load = loader(async ({ params }: LoaderArgs & { params: { slug: string } }): Promise<{ slug: string; render: number }> => ({
  slug: params.slug,
  render: render(),
}));

export default function Post(props: { data: { slug: string; render: number } }) {
  return (
    <main>
      <h1>Post {props.data.slug}</h1>
      <p>Render {props.data.render}</p>
    </main>
  );
}
