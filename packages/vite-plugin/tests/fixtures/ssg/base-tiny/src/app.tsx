import publicLogo from "/brand.svg";
import { defineRoute, defineRoutes, useLocation, useNavigate } from "@rezejs/router";
import type { JSX } from "reze-js";

import { PackageBadge } from "../../linked-ui/src";
import logo from "./logo café.svg?no-inline";
import logo2x from "./logo café@2x.svg?no-inline";

import "./styles.css";

function Home() {
  return <h1 id="tiny-home">tiny home</h1>;
}

function Deep() {
  return <h1 id="tiny-deep">tiny deep</h1>;
}

export const routes = defineRoutes([
  defineRoute({ path: "/", meta: { title: "Tiny" }, component: Home }),
  defineRoute({ path: "/a/b", meta: { title: "Deep" }, component: Deep }),
]);

export default function Shell(props: { children: JSX.Element }) {
  const location = useLocation();
  const navigate = useNavigate();
  return (
    <>
      <p id="tiny-path" ref={(node) => node.setAttribute("data-hydrated", "")}>
        {location().pathname}
      </p>
      <button id="tiny-go-home" type="button" onClick={() => navigate("/")}>
        home
      </button>
      <img
        id="tiny-logo"
        src={new URL("./logo%20café.svg?no-inline", import.meta.url).href}
        srcset={`${logo} 1x, ${logo2x} 2x`}
        alt="logo"
      />
      <img id="tiny-public" src={publicLogo} alt="public logo" />
      <PackageBadge />
      <main>{props.children}</main>
    </>
  );
}
