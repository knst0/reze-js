import { expect, test } from "../fixtures";

test("streams deferred fields into their boundaries", async ({ page, server }) => {
  const response = page.waitForResponse(server.url("/stream"));
  await page.goto(server.url("/stream"), { waitUntil: "commit" });
  await expect(page.getByRole("heading", { name: "Stream" })).toBeVisible();
  expect((await response).headers().etag).toBeUndefined();
  await expect(page.getByRole("button", { name: "Likes 7" })).toBeVisible();
  await expect(page.getByRole("list", { name: "Comments" }).getByRole("listitem")).toHaveText(["First!", "Second"]);
  await expect(page.getByText("Loading comments…")).toHaveCount(0);
  await expect(page.locator("template")).toHaveCount(0);
  await page.getByRole("button", { name: "Likes 7" }).click();
  await expect(page.getByRole("button", { name: "Likes 8" })).toBeVisible();
});

test("streams deferred fields after a client navigation", async ({ page, server }) => {
  await page.goto(server.url("/login"));
  await page.evaluate(() => ((window as unknown as { kept: boolean }).kept = true));
  await page.getByRole("link", { name: "Stream" }).click();
  await expect(page.getByRole("list", { name: "Comments" }).getByRole("listitem")).toHaveText(["First!", "Second"]);
  await page.getByRole("button", { name: "Likes 7" }).click();
  await expect(page.getByRole("button", { name: "Likes 8" })).toBeVisible();
  expect(await page.evaluate(() => (window as unknown as { kept?: boolean }).kept)).toBe(true);
});

test("keeps deferred parts in templates without JS", async ({ browser, server }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto(server.url("/stream"));
  await expect(page.getByText("Loading comments…")).toBeVisible();
  await expect(page.getByRole("list", { name: "Comments" })).toHaveCount(0);
  await context.close();
});

test("answers conditional requests with 304", async ({ request, server }) => {
  const first = await request.get(server.url("/login?next=todos"));
  const etag = first.headers().etag!;
  expect(etag).toMatch(/^".+"$/);
  const second = await request.get(server.url("/login?next=todos"), { headers: { "if-none-match": etag } });
  expect(second.status()).toBe(304);
});
