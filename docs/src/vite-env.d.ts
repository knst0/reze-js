/// <reference types="vite/client" />

declare module "*.mdx" {
  import type { JSX } from "reze-js";
  export const frontmatter: { title: string; description: string };
  export const route: { meta: { title: string; description: string } };
  const Content: (props: { children?: JSX.Element; [key: string]: unknown }) => JSX.Element;
  export default Content;
}

declare module "disarto-icons/icons/*" {
  const icon: {
    readonly name: string;
    readonly viewBox: string;
    readonly inner: string;
    toSvg(attrs?: Record<string, string>): string;
  };
  export default icon;
}
