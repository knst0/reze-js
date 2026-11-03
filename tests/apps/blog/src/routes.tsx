export default function Root(props: { children: unknown }) {
  return (
    <div>
      <head>
        <title>Blog</title>
      </head>
      <nav>
        <a href="/">Home</a>
      </nav>
      {props.children}
    </div>
  );
}
