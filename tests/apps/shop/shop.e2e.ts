import { expect, test } from "../fixtures";

for (const javaScriptEnabled of [true, false]) {
  test.describe(javaScriptEnabled ? "with JS" : "without JS", () => {
    test.use({ javaScriptEnabled });

    test("renders the catalog and fills the cart", async ({ page, server }) => {
      await page.goto(server.url("/"));
      await expect(page).toHaveTitle("Summit Outfitters");
      await expect(page.getByRole("list", { name: "Products" }).getByRole("listitem")).toHaveCount(24);
      await expect(page.getByLabel("Cart")).toHaveText("0 in cart");
      await page.getByRole("button", { name: "Add Trail Runner 2 to cart" }).click();
      await expect(page.getByLabel("Cart")).toHaveText("1 in cart");
      await page.getByRole("button", { name: "Add Water Filter to cart" }).click();
      await expect(page.getByLabel("Cart")).toHaveText("2 in cart");
      await expect(page).toHaveURL(server.url("/"));
    });
  });
}

test("compares products on the client", async ({ page, server }) => {
  await page.goto(server.url("/"));
  const compare = page.getByRole("button", { name: "Compare Compass" });
  await compare.click();
  await expect(compare).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("button", { name: "Compare Gaiters" })).toHaveAttribute("aria-pressed", "false");
});

test("opens a product", async ({ page, server }) => {
  await page.goto(server.url("/"));
  await page.getByRole("link", { name: "Water Filter" }).click();
  await expect(page).toHaveURL(server.url("/products/8"));
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Water Filter");
});

test("picks a view and a quantity on the product page", async ({ page, server }) => {
  await page.goto(server.url("/products/8"));
  await page.getByRole("button", { name: "side" }).click();
  await expect(page.getByRole("img", { name: "Water Filter, side view" })).toBeVisible();
  await expect(page.getByRole("button", { name: "side" })).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("button", { name: "More" }).click();
  await page.getByRole("button", { name: "More" }).click();
  await page.getByRole("button", { name: "Fewer" }).click();
  await expect(page.getByLabel("Quantity")).toHaveText("2");
  await page.getByRole("button", { name: "Add to cart" }).click();
  await expect(page.getByLabel("Cart")).toHaveText("2 in cart");
  await expect(page.getByRole("article")).toHaveCount(8);
});
