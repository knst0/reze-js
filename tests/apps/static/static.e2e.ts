import { expect, test } from "../fixtures";

test.use({ javaScriptEnabled: false });

test("renders a list", async ({ page, server }) => {
  await page.goto(server.url("/"));
  await expect(page).toHaveTitle("Static");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Todos");
  await expect(page.getByRole("listitem")).toHaveText(["Write the spec", "Ship the compiler"]);
  await expect(page.getByRole("listitem").first()).toHaveClass("done");
  await expect(page.getByText("1 of 2 done")).toBeVisible();
});

test("renders a param route under the layout", async ({ page, server }) => {
  await page.goto(server.url("/"));
  await page.getByRole("link", { name: "Ship the compiler" }).click();
  await expect(page).toHaveURL(server.url("/todos/2"));
  await expect(page).toHaveTitle("Todo 2");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Todo 2");
  await expect(page.getByRole("link", { name: "Home" })).toBeVisible();
});

test("answers an unknown URL with 404", async ({ page, server }) => {
  const response = await page.goto(server.url("/missing"));
  expect(response?.status()).toBe(404);
});
