export default function Root(props: { children: unknown }) {
  return (
    <div>
      <head>
        <title>Static</title>
      </head>
      <nav>
        <a href="/">Home</a>
      </nav>
      {props.children}
    </div>
  );
}
