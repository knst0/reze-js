function Seo(props: { title: string }) {
  return (
    <>
      <head>
        <title>{props.title}</title>
        <meta name="description" content="from component" />
        <link rel="stylesheet" href="/a.css" />
      </head>
      <span>seo</span>
    </>
  );
}

export default function Home() {
  return (
    <div>
      <head>
        <title>Page</title>
        <link rel="stylesheet" href="/a.css" />
      </head>
      <Seo title="Child" />
    </div>
  );
}
