export default function Note(props: { params: { slug: string } }) {
  return (
    <article>
      <head>
        <title>Note {props.params.slug}</title>
      </head>
      <h1>Note {props.params.slug}</h1>
      <a href="/notes/second">Second note</a>
    </article>
  );
}
