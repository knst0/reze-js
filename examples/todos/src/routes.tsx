import { Errored, type JSX } from "reze-js";

export default function Root(props: { children: JSX.Element }) {
  return (
    <>
      <head>
        <title>Todos</title>
      </head>
      <nav>
        <a href="/">Todos</a> <a href="/about">About</a>
      </nav>
      <Errored
        fallback={(error) => (
          <p role="alert">
            {error.status} {error.message}
          </p>
        )}
      >
        {props.children}
      </Errored>
    </>
  );
}
