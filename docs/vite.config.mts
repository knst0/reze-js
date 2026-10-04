import mdx from "@mdx-js/rollup";
import reze, { DEFAULT_ROUTE_EXTENSIONS } from "@rezejs/vite-plugin";
import tailwindcss from "@tailwindcss/vite";
import rehypeSlug from "rehype-slug";
import remarkFrontmatter from "remark-frontmatter";
import remarkGfm from "remark-gfm";
import remarkMdxFrontmatter from "remark-mdx-frontmatter";
import { defineConfig } from "vite";

import frontmatterRoute from "./plugins/frontmatter-route";
import llms from "./plugins/llms";
import rehypeSugarHigh from "./plugins/rehype-sugar-high";

export default defineConfig({
  plugins: [
    {
      ...mdx({
        jsx: true,
        jsxImportSource: "reze-js",
        providerImportSource: "/src/mdx",
        remarkPlugins: [remarkFrontmatter, remarkMdxFrontmatter, frontmatterRoute, remarkGfm],
        rehypePlugins: [rehypeSlug, rehypeSugarHigh],
      }),
      enforce: "pre",
    },
    reze({ fileRoutes: true, extensions: [...DEFAULT_ROUTE_EXTENSIONS, ".mdx"], ssg: { entry: "src/app.tsx" } }),
    tailwindcss(),
    llms(),
  ],
  server: {
    port: 3000,
  },
  build: {
    target: "esnext",
    assetsInlineLimit: 0,
    cssMinify: "lightningcss",
    modulePreload: { polyfill: false },
  },
});
