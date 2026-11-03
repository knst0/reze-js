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

test("starts an effect when its element enters the viewport", async ({ page, server }) => {
  const requested: string[] = [];
  page.on("request", (request) => {
    if (chunk.test(request.url())) requested.push(request.url());
  });
  await page.goto(server.url("/ticker"));
  await page.evaluate(() => new Promise((resolve) => (window.requestIdleCallback ?? setTimeout)(resolve)));
  expect(requested).toEqual([]);
  await expect(page.getByText("Waiting")).toBeAttached();
  await page.getByText("Waiting").scrollIntoViewIfNeeded();
  await expect(page.getByText("Started")).toBeVisible();
  expect(requested).toHaveLength(1);
});

test("writes a store field without updating the readers of another", async ({ page, server }) => {
  await page.goto(server.url("/store"));
  await page.getByRole("button", { name: "Pear" }).click();
  await expect(page.getByLabel("Pears")).toHaveText("1");
  await page.evaluate(() => {
    const pears = document.querySelector("[aria-label=Pears]")!;
    const records: MutationRecord[] = [];
    new MutationObserver((list) => records.push(...list)).observe(pears, { characterData: true, childList: true, subtree: true });
    (window as unknown as { records: MutationRecord[] }).records = records;
  });
  await page.getByRole("button", { name: "Apple" }).click();
  await expect(page.getByLabel("Apples")).toHaveText("1");
  expect(await page.evaluate(() => (window as unknown as { records: MutationRecord[] }).records.length)).toBe(0);
  const todos = page.getByRole("list", { name: "Todos" }).getByRole("listitem");
  await page.getByRole("button", { name: "Push" }).click();
  await page.getByRole("button", { name: "Finish" }).click();
  await expect(todos).toHaveText(["Milk done", "Todo 2 open"]);
  await expect(page.getByLabel("Count")).toHaveText("2 todos");
  await page.getByRole("button", { name: "Clear" }).click();
  await expect(todos).toHaveCount(0);
  await expect(page.getByLabel("Count")).toHaveText("0 todos");
});

test.describe("lists", () => {
  const todos = (page: import("@playwright/test").Page) => page.getByRole("list", { name: "Todos" }).getByRole("listitem");

  test("toggles a client range", async ({ page, server }) => {
    await page.goto(server.url("/lists"));
    await expect(page.getByText("Hidden details")).toBeVisible();
    await page.getByRole("button", { name: "Details" }).click();
    await expect(page.getByText("Shown with 2 todos")).toBeVisible();
    await expect(page.getByText("Hidden details")).toBeHidden();
    await page.getByRole("button", { name: "Star" }).click();
    await expect(page.getByLabel("Stars").locator("span")).toHaveCount(2);
    await page.getByRole("button", { name: "Push" }).click();
    await expect(page.getByText("Shown with 3 todos")).toBeVisible();
    await page.getByRole("button", { name: "Details" }).click();
    await expect(page.getByText("Hidden details")).toBeVisible();
  });

  test("updates the rows of a list", async ({ page, server }) => {
    await page.goto(server.url("/lists"));
    await page.getByRole("button", { name: "Push" }).click();
    await expect(todos(page)).toHaveCount(3);
    await expect(todos(page).nth(2)).toContainText("3. Item 3");
    await expect(page.getByLabel("Tags")).toHaveText("MilkBreadItem 3");
    await page.getByRole("button", { name: "Reverse" }).click();
    await expect(todos(page).nth(0)).toContainText("1. Item 3");
    await expect(todos(page).nth(2)).toContainText("3. Milk");
    await expect(page.getByLabel("Tags")).toHaveText("Item 3BreadMilk");
    await page.getByRole("button", { name: "Pick Bread" }).click();
    const bread = todos(page).filter({ hasText: "Bread" });
    await expect(bread).toHaveAttribute("aria-selected", "true");
    await expect(bread.getByText("picked")).toBeVisible();
    await page.getByRole("button", { name: "Pick Item 3" }).click();
    await expect(bread).toHaveAttribute("aria-selected", "false");
    await expect(page.getByText("picked")).toHaveCount(1);
    await page.getByRole("button", { name: "Drop Item 3" }).click();
    await expect(todos(page).nth(0)).toContainText("1. Bread");
    await page.getByRole("button", { name: "Drop Bread" }).click();
    await page.getByRole("button", { name: "Drop Milk" }).click();
    await expect(todos(page)).toHaveText(["Nothing to do"]);
    await expect(page.getByLabel("Tags")).toHaveText("");
    await page.getByRole("button", { name: "Push" }).click();
    await expect(todos(page)).toHaveCount(1);
    await expect(todos(page).nth(0)).toContainText("1. Item 1");
  });
});
