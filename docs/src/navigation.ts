import type { Href } from "@rezejs/router";
import { paths } from "virtual:reze-routes";

export interface NavPage {
  href: Href;
  label: string;
}

export interface NavSection {
  title?: string;
  pages: NavPage[];
}

export const sections: NavSection[] = [
  { pages: [{ href: paths.installation(), label: "Getting started" }] },
  {
    title: "Core concepts",
    pages: [
      { href: paths.components(), label: "Components and JSX" },
      { href: paths.reactivity(), label: "Reactivity" },
      { href: paths.lifecycle(), label: "Lifecycle and cleanup" },
      { href: paths["familiar-patterns"](), label: "Familiar patterns" },
    ],
  },
  {
    title: "Building applications",
    pages: [
      { href: paths["building-interfaces"](), label: "Building interfaces" },
      { href: paths.routing(), label: "Routing" },
    ],
  },
  {
    title: "Reference",
    pages: [
      { href: paths["router-api"](), label: "Router API" },
      { href: paths["vite-plugin"](), label: "Vite plugin" },
      { href: paths.compiler(), label: "Compiler" },
    ],
  },
];

export const pages: NavPage[] = sections.flatMap((section) => section.pages);
