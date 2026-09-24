import { useLocation } from "@rezejs/router";

export default function NotFound() {
  return <p>Nothing at {useLocation().pathname}.</p>;
}
