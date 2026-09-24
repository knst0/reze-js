import { useParams } from "../../../../src";

export default function User() {
  const params = useParams();
  return <p>user {params.id}</p>;
}
