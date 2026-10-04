import { useNavigate } from "@rezejs/router";

export default function Index() {
  useNavigate()("/installation", { replace: true });
  return null;
}
