import { useParams } from "../../../../src";

export default function Docs() {
  const params = useParams();
  return <p>docs {params.page ?? "index"}</p>;
}
