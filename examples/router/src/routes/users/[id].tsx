import { Title } from "@rezejs/meta";
import { useParams } from "@rezejs/router";

const names: Record<string, string> = { "1": "Ada Lovelace", "2": "Grace Hopper" };

export default function User() {
  const params = useParams();
  const name = () => names[params.id ?? ""] ?? "Unknown user";
  return (
    <>
      <Title>Reze · {name()}</Title>
      <p>{name()}</p>
    </>
  );
}
