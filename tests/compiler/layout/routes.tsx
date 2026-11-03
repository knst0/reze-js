export default function Root(props: { children: unknown }) {
  return (
    <body-shell>
      <head>
        <title>Root</title>
      </head>
      <nav>nav</nav>
      {props.children}
    </body-shell>
  );
}
