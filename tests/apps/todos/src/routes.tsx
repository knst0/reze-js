import { Errored, type JSX } from "reze-js";

export default function Root(props: { children: JSX.Element }) {
  return (
    <div>
      <head>
        <title>Todos</title>
      </head>
      <nav>
        <a href="/todos">Todos</a>
        <a href="/stream">Stream</a>
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
    </div>
  );
}
