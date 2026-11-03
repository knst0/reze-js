import { expect, test } from "../fixtures";

const paths = ["/", "/posts/1", "/posts/2", "/posts/9", "/posts/abc", "/missing"];

test("renders the same HTML on the Rust and the JS server", async ({ request, server }) => {
  for (const path of paths) {
    const [rust, js] = await Promise.all([request.get(server.url(path)), request.get(server.js!(path))]);
    expect(rust.status(), path).toBe(js.status());
    expect(await rust.text(), path).toBe(await js.text());
  }
});

test("writes Server-Timing with the dev feature", async ({ request, server }) => {
  const timing = (await request.get(server.url("/posts/1"))).headers()["server-timing"];
  expect(timing).toMatch(/^loader-1;dur=\d+\.\d, render;dur=\d+\.\d$/);
});

test.describe("without JS", () => {
  test.use({ javaScriptEnabled: false });

  test("renders Rust loader data", async ({ page, server }) => {
    await page.goto(server.url("/"));
    await expect(page).toHaveTitle("Blog");
    await expect(page.getByRole("listitem")).toHaveText(["1. Hello, world Published", '2. Drafts "quoted" Draft']);
    await expect(page.getByRole("listitem").first()).toHaveClass("Published");
    await expect(page.getByText("2 posts")).toBeVisible();
  });

  test("renders a param route", async ({ page, server }) => {
    await page.goto(server.url("/posts/1"));
    await expect(page).toHaveTitle("Hello, world");
    await expect(page.locator(".subtitle")).toHaveText("The first post");
    await expect(page.getByText("The body of Hello, world <with markup> & more.")).toBeVisible();
    await expect(page.getByRole("list", { name: "Tags" }).getByRole("listitem")).toHaveText(["intro", "rust"]);
    await expect(page.getByText("Rated 4.5, 12345678901234 views")).toBeVisible();
    await expect(page.getByText("Published", { exact: true })).toBeVisible();
    await expect(page.getByText("Tagged", { exact: true })).toBeVisible();
  });

  test("renders optional and empty fields", async ({ page, server }) => {
    await page.goto(server.url("/posts/2"));
    await expect(page.locator(".subtitle")).toHaveCount(0);
    await expect(page.getByRole("list", { name: "Tags" }).getByRole("listitem")).toHaveText(["Untagged"]);
    await expect(page.getByText("Rated 0.30000000000000004, 0 views")).toBeVisible();
    await expect(page.getByText("Draft", { exact: true })).toBeVisible();
    await expect(page.getByText("Tagged", { exact: true })).toHaveCount(0);
  });

  test("answers a loader's not found and an unknown URL with 404", async ({ page, server }) => {
    expect((await page.goto(server.url("/posts/9")))?.status()).toBe(404);
    expect((await page.goto(server.url("/missing")))?.status()).toBe(404);
  });
});

test("runs a handler over Rust data", async ({ page, server }) => {
  await page.goto(server.url("/posts/1"));
  await expect(page.locator("output")).toHaveText("2 likes");
  await page.getByRole("button", { name: "Like" }).click();
  await expect(page.locator("output")).toHaveText("3 likes");
});

test("navigates between Rust pages", async ({ page, server }) => {
  await page.goto(server.url("/"));
  await page.getByRole("link", { name: "Hello, world" }).click();
  await expect(page).toHaveURL(server.url("/posts/1"));
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Hello, world");
  await page.getByRole("link", { name: "Home" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Blog");
});

test("marks the link to the current page", async ({ page, server }) => {
  await page.goto(server.url("/"));
  const home = page.getByRole("link", { name: "Home" });
  await expect(home).toHaveAttribute("aria-current", "page");
  await expect(home).toHaveAttribute("data-active", "");
  await page.getByRole("link", { name: "Hello, world" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Hello, world");
  await expect(home).not.toHaveAttribute("aria-current");
  await expect(home).not.toHaveAttribute("data-active");
  await page.goBack();
  await expect(home).toHaveAttribute("aria-current", "page");
});
