import { join } from "node:path";

import { expect, test } from "vitest";

import { headingAnchors, internalLinks, readPages } from "./pages";

const pages = await readPages(join(import.meta.dirname, "../src/routes"));
const anchorsByRoute = new Map(pages.map((page) => [page.route, headingAnchors(page)]));

test.each(pages.map((page) => ({ name: page.name, links: internalLinks(page) })))(
  "$name links to existing pages and headings",
  ({ links }) => {
    const broken = links.filter((url) => {
      const { pathname, hash } = new URL(url, "https://docs.invalid");
      const anchors = anchorsByRoute.get(pathname.replace(/\/$/, "") || "/");
      return anchors === undefined || (hash !== "" && !anchors.has(decodeURIComponent(hash.slice(1))));
    });
    expect(broken).toEqual([]);
  },
);
