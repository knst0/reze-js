import type { JSX } from "reze-js";

export const route = { render: "client" as const };

export default function Dashboard(props: { children: JSX.Element }) {
  return (
    <section id="dashboard">
      <h1>dashboard {typeof window === "undefined" ? "server" : "browser"}</h1>
      <a id="dashboard-settings" href="/dashboard/settings">
        settings
      </a>{" "}
      <a id="dashboard-about" href="/about">
        about
      </a>
      {props.children}
    </section>
  );
}
