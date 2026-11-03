import { loader } from "reze-js/server";

import { render } from "../db";

export const load = loader(async (): Promise<{ render: number }> => ({ render: render() }));

export default function About(props: { data: { render: number } }) {
  return (
    <main>
      <h1>About</h1>
      <p>Render {props.data.render}</p>
    </main>
  );
}
