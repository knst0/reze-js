import { useNavigate } from "@rezejs/router";

export default function AdminPage() {
  useNavigate()("/about", { replace: true });
  return <p>admin</p>;
}
