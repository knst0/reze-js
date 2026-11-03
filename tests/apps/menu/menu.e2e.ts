import { expect, test } from "../fixtures";

const chunk = /virtual:rz-route\/|\/route-\d+-/;

test.describe("without JS", () => {
  test.use({ javaScriptEnabled: false });

  test("renders the page and follows links", async ({ page, server }) => {
    await page.goto(server.url("/"));
    await expect(page.getByRole("button", { name: "Menu" })).toHaveAttribute("aria-expanded", "false");
    await expect(page.getByRole("link", { name: "Docs" })).toBeHidden();
    await expect(page.getByText("Clicked 0 times")).toBeVisible();
    await page.getByRole("link", { name: "Stay here" }).click();
    await expect(page.getByRole("heading", { name: "Elsewhere" })).toBeVisible();
  });
});

test("toggles the menu", async ({ page, server }) => {
  await page.goto(server.url("/"));
  const button = page.getByRole("button", { name: "Menu" });
  await button.click();
  await expect(button).toHaveAttribute("aria-expanded", "true");
  await expect(page.getByRole("link", { name: "Docs" })).toBeVisible();
  await button.click();
  await expect(page.getByRole("link", { name: "Docs" })).toBeHidden();
});

test("updates text in place", async ({ page, server }) => {
  await page.goto(server.url("/"));
  const add = page.getByRole("button", { name: "Add" });
  await add.click();
  await add.click();
  await expect(page.getByText("Clicked 2 times")).toBeVisible();
});

test("selects a row", async ({ page, server }) => {
  await page.goto(server.url("/"));
  const pear = page.getByRole("listitem").filter({ hasText: "Pear" });
  await pear.click();
  await expect(pear).toHaveAttribute("aria-selected", "true");
  const plum = page.getByRole("listitem").filter({ hasText: "Plum" });
  await plum.click();
  await expect(plum).toHaveAttribute("aria-selected", "true");
  await expect(pear).toHaveAttribute("aria-selected", "false");
});

test("prevents the default of a link", async ({ page, server }) => {
  await page.goto(server.url("/"));
  await page.getByRole("link", { name: "Stay here" }).click();
  await expect(page.getByText("Stayed")).toBeVisible();
  await expect(page).toHaveURL(server.url("/"));
});

test("replays a click made while the chunk loads", async ({ page, server }) => {
  let release!: () => void;
  const released = new Promise<void>((resolve) => (release = resolve));
  await page.route(chunk, async (route) => {
    await released;
    await route.continue();
  });
  await page.goto(server.url("/"));
  await page.getByRole("button", { name: "Add" }).click();
  await page.getByRole("link", { name: "Stay here" }).click();
  await expect(page).toHaveURL(server.url("/"));
  await expect(page.getByText("Clicked 0 times")).toBeVisible();
  release();
  await expect(page.getByText("Clicked 1 times")).toBeVisible();
  await expect(page.getByText("Stayed")).toBeVisible();
});

test("falls back to the native action when the chunk fails", async ({ page, server }) => {
  await page.route(chunk, (route) => route.abort());
  await page.goto(server.url("/"));
  await page.getByRole("link", { name: "Stay here" }).click();
  await expect(page.getByRole("heading", { name: "Elsewhere" })).toBeVisible();
});

test("reloads once when a chunk fails without a native action", async ({ page, server }) => {
  await page.route(chunk, (route) => route.abort());
  await page.goto(server.url("/"));
  const reloaded = page.waitForEvent("load");
  await page.getByRole("button", { name: "Add" }).click();
  await reloaded;
  await page.getByRole("button", { name: "Add" }).click();
  await expect(page.getByText("Clicked 0 times")).toBeVisible();
  expect(await page.evaluate(() => sessionStorage.getItem("rz:reload"))).toBe("1");
});
