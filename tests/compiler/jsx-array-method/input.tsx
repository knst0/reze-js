export default function Home() {
  const tags = [{ name: "a" }, { name: "b" }];
  return <span>{tags.map((tag) => tag.name).join(", ")}</span>;
}
