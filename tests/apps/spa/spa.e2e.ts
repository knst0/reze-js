import { expect, test } from "../fixtures";

test("renders the page in the browser from a static shell", async ({ page, server }) => {
  const shell = await (await fetch(server.url("/notes/first"))).text();
  expect(shell).not.toContain("<h1>");
  await page.goto(server.url("/"));
  await expect(page.getByRole("heading", { name: "Home" })).toBeVisible();
  await page.getByRole("button", { name: "Add" }).click();
  await page.getByRole("button", { name: "Add" }).click();
  await expect(page.locator("output")).toHaveText("2");
  await expect(page.getByText("Doubled: 4")).toBeVisible();
});

test("navigates between routes and keeps the layout's state", async ({ page, server }) => {
  await page.goto(server.url("/"));
  await page.getByRole("button", { name: "Visits 0" }).click();
  await page.getByRole("link", { name: "First note" }).click();
  await expect(page.getByRole("heading", { name: "Note first" })).toBeVisible();
  await expect(page).toHaveURL(server.url("/notes/first"));
  await expect(page).toHaveTitle("Note first");
  await expect(page.getByRole("link", { name: "First note" })).toHaveAttribute("aria-current", "page");
  await expect(page.getByRole("link", { name: "Home" })).not.toHaveAttribute("aria-current");
  await page.getByRole("link", { name: "Second note" }).click();
  await expect(page.getByRole("heading", { name: "Note second" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Visits 1" })).toBeVisible();
  await page.goBack();
  await expect(page.getByRole("heading", { name: "Note first" })).toBeVisible();
  await page.getByRole("button", { name: "Visits 1" }).click();
  await expect(page.getByRole("button", { name: "Visits 2" })).toBeVisible();
});

test("opens a deep link and renders 404 for an unknown URL", async ({ page, server }) => {
  await page.goto(server.url("/notes/third"));
  await expect(page.getByRole("heading", { name: "Note third" })).toBeVisible();
  await page.goto(server.url("/missing"));
  await expect(page.getByText("Not Found")).toBeVisible();
});
