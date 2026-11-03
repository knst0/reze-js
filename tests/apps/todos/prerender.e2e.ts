import type { Page } from "@playwright/test";

import { expect, test, type Server } from "../fixtures";

const rendered = async (page: Page, path: string, server: Server) => {
  await page.goto(server.url(path));
  return Number((await page.getByText(/^Render \d+$/).textContent())!.slice(7));
};

test("prerenders static routes and listed entries in the build", async ({ page, server, serverKind }) => {
  for (const path of ["/about", "/posts/first"]) {
    const first = await rendered(page, path, server);
    const second = await rendered(page, path, server);
    if (serverKind === "js-build") expect(second).toBe(first);
    else expect(second).toBeGreaterThan(first);
  }
  const first = await rendered(page, "/posts/other", server);
  expect(await rendered(page, "/posts/other", server)).toBeGreaterThan(first);
});

test("turns revalidate into shared-cache freshness", async ({ request, server }) => {
  const response = await request.get(server.url("/posts/other"));
  expect(response.headers()["cache-control"]).toBe("s-maxage=60, stale-while-revalidate");
});
