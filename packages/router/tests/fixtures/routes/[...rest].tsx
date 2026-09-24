import { useParams } from "../../../src";

export default function NotFound() {
  const params = useParams();
  return <p>missing {params.rest}</p>;
}
