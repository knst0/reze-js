/// <reference types="vite/client" />

declare module "*.mdx" {
  import type { JSX } from "reze-js";
  export const frontmatter: { title: string; description: string };
  const Content: (props: { children?: JSX.Element; [key: string]: unknown }) => JSX.Element;
  export default Content;
}
