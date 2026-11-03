import { dynamic } from "reze-js";

export default async function Home(props: { path: string }) {
  const Widget = dynamic(await import(props.path));
  return <Widget />;
}
