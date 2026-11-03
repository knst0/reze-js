import { type JSX, signal } from "reze-js";

export default function Root(props: { children: JSX.Element }) {
  let visits = signal(0);
  return (
    <div>
      <head>
        <title>Notes</title>
      </head>
      <nav>
        <a href="/">Home</a>
        <a href="/notes/first">First note</a>
        <button onClick={() => visits++}>Visits {visits}</button>
      </nav>
      <main>{props.children}</main>
    </div>
  );
}
