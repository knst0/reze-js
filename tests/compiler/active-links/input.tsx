export default function Home() {
  const about = "/about";
  const attrs = { href: "/" };
  return (
    <nav>
      <a href="/">Home</a>
      <a href="/?tab=new#top">Home, new</a>
      <a href={about}>About</a>
      <a href="/" aria-current="location">Pinned</a>
      <a {...attrs}>Spread</a>
      <a href="//example.com/">Other origin</a>
      <a href="https://example.com/">External</a>
    </nav>
  );
}
