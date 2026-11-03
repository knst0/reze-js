import { expect, test } from "../fixtures";

test("leaves a client-rendered route empty in the document", async ({ server }) => {
  const document = await (await fetch(server.url("/editor"))).text();
  expect(document).toContain("<nav>");
  expect(document).toContain("<template data-rz-client></template>");
  expect(document).not.toContain("<h1>Editor</h1>");
});

test("renders a client-rendered route in the browser", async ({ page, server }) => {
  await page.goto(server.url("/editor"));
  await expect(page.getByRole("heading", { name: "Editor" })).toBeVisible();
  await expect(page.getByText(/^Render \d+$/)).toBeVisible();
  await page.getByRole("button", { name: "Words 0" }).click();
  await expect(page.getByRole("button", { name: "Words 1" })).toBeVisible();
});

test("renders a client-rendered route on a client navigation", async ({ page, server }) => {
  await page.goto(server.url("/about"));
  await page.evaluate(() => {
    const link = Object.assign(document.createElement("a"), { href: "/editor", textContent: "Editor" });
    document.querySelector("nav")!.append(link);
  });
  await page.getByRole("link", { name: "Editor" }).click();
  await expect(page.getByRole("heading", { name: "Editor" })).toBeVisible();
  await page.getByRole("button", { name: "Words 0" }).click();
  await expect(page.getByRole("button", { name: "Words 1" })).toBeVisible();
});

test("restores dates, bigints, sets, and maps from the state block", async ({ page, server }) => {
  await page.goto(server.url("/about"));
  await page.getByRole("button", { name: "Facts" }).click();
  await expect(page.locator("output")).toHaveText("2024 9007199254740994 true 2020");
});
