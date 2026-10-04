import type { JSX } from "reze-js";
import { defineRoute, defineRoutes, useLocation, useNavigate } from "@rezejs/router";

import "./styles.css";

export const routes = defineRoutes([
  defineRoute({ path: "/", meta: { title: "Tiny" }, component: () => <h1 id="tiny-home">tiny home</h1> }),
  defineRoute({ path: "/a/b", meta: { title: "Deep" }, component: () => <h1 id="tiny-deep">tiny deep</h1> }),
]);

export default function Shell(props: { children: JSX.Element }) {
  const location = useLocation();
  const navigate = useNavigate();
  return (
    <>
      <p id="tiny-path" ref={node => node.setAttribute("data-hydrated", "")}>{location().pathname}</p>
      <button id="tiny-go-home" type="button" onClick={() => navigate("/")}>home</button>
      <main>{props.children}</main>
    </>
  );
}
