export default function Home() {
  const first = "Ann";
  const last = "Lee";
  const count = 0;
  const html = "<b>&</b>";
  return (
    <p title={`${first} "${last}"`} hidden={false} data-n={count}>
      {first}
      {last} has {count} items. {html}
      {null}
      {true}
      {"`${template}`"}
    </p>
  );
}
