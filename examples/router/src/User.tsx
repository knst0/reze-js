import { useParams } from "@rezejs/router";

const names: Record<string, string> = { "1": "Ada Lovelace", "2": "Grace Hopper" };

export default function User() {
  const params = useParams();
  return <p>{names[params.id ?? ""] ?? "Unknown user"}</p>;
}
