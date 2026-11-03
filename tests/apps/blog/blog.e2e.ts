import { expect, test } from "../fixtures";

const paths = ["/", "/?ref=mail&_s=x", "/posts/1?ref=feed", "/posts/1", "/posts/2", "/posts/9", "/posts/abc", "/posts/members", "/missing"];

test("renders the same HTML on the Rust and the JS server", async ({ request, server }) => {
  for (const path of paths) {
    const [rust, js] = await Promise.all([request.get(server.url(path)), request.get(server.js!(path))]);
    expect(rust.status(), path).toBe(js.status());
    expect(await rust.text(), path).toBe(await js.text());
  }
});

test("redirects from a Rust loader", async ({ request, server }) => {
  for (const url of [server.url("/posts/first"), server.js!("/posts/first")]) {
    const response = await request.get(url, { maxRedirects: 0 });
    expect(response.status()).toBe(307);
    expect(response.headers().location).toBe("/posts/1");
  }
});

test("runs a Rust guard before the loaders", async ({ request, server }) => {
  for (const url of [(path: string) => server.url(path), (path: string) => server.js!(path)]) {
    const denied = await request.get(url("/posts/members"));
    expect(denied.status()).toBe(403);
    expect(await denied.text()).toContain("Members only");
    expect((await request.get(url("/posts/members"), { headers: { "x-member": "1" } })).status()).toBe(404);
    const login = await request.get(url("/posts/members?login"), { maxRedirects: 0 });
    expect(login.status()).toBe(307);
    expect(login.headers().location).toBe("/");
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
  await expect(page.getByLabel("Hearts")).toHaveText("Liked");
  await page.getByRole("button", { name: "Like" }).click();
  await expect(page.locator("output")).toHaveText("3 likes");
  await expect(page.getByLabel("Hearts")).toHaveText("Loved");
});

test("sends Rust data to the client in the positional encoding", async ({ page, request, server }) => {
  const html = await (await request.get(server.url("/"))).text();
  expect(/<script type="rz">(.*?)<\/script>/.exec(html)![1]).toContain('[[1,"Hello, world",1],[2,"Drafts \\"quoted\\"",0]]');
  await page.goto(server.url("/"));
  await page.getByRole("button", { name: "Count drafts" }).click();
  await expect(page.locator("output")).toHaveText("1 drafts");
});

test("loads each component from its own chunk", async ({ page, request, server }) => {
  const html = await (await request.get(server.url("/posts/1"))).text();
  const [, chunks] = JSON.parse(/<script type="rz">(.*?)<\/script>/.exec(html)![1]!) as [string, Record<number, string>];
  expect(new Set(Object.values(chunks)).size).toBe(3);
  await page.goto(server.url("/posts/1"));
  await page.getByRole("button", { name: "Share" }).click();
  await expect(page.getByText("Link copied")).toBeVisible();
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

test("reads the location through router hooks", async ({ page, server }) => {
  await page.goto(server.url("/?ref=mail"));
  const where = page.locator(".where");
  await expect(where).toHaveText("At /?ref=mail from mail");
  await expect(where).toHaveAttribute("data-home", "");
  await page.getByRole("link", { name: "Hello, world" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Hello, world");
  await expect(where).toHaveText("At /posts/1");
  await expect(where).not.toHaveAttribute("data-home");
});

test("updates client ranges over Rust data", async ({ page, server }) => {
  await page.goto(server.url("/posts/1"));
  const notes = page.getByRole("list", { name: "Notes" }).getByRole("listitem");
  await expect(page.getByText("Few likes")).toBeVisible();
  await page.getByRole("button", { name: "Like" }).click();
  await expect(page.getByText("Popular with 3 likes")).toBeVisible();
  await page.getByRole("button", { name: "Like" }).click();
  await expect(page.getByText("Popular with 4 likes")).toBeVisible();
  await page.getByRole("button", { name: "Note" }).click();
  await expect(notes).toHaveText(["1: introDrop intro", "2: rustDrop rust", "3: note 3Drop note 3"]);
  await page.getByRole("button", { name: "Drop intro" }).click();
  await expect(notes).toHaveText(["1: rustDrop rust", "2: note 3Drop note 3"]);
  await expect(page.getByLabel("Words")).toHaveText("rustnote 3");
});

for (const name of ["Break", "Break later"]) {
  test(`renders the boundary's fallback when a handler fails: ${name}`, async ({ page, server }) => {
    await page.goto(server.url("/posts/1"));
    await page.getByRole("button", { name: "Report", exact: true }).click();
    await expect(page.getByText("1 reports")).toBeVisible();
    await page.getByRole("button", { name, exact: true }).click();
    await expect(page.getByRole("alert")).toHaveText("Report failed with 500");
    await expect(page.getByRole("button", { name: "Report", exact: true })).toHaveCount(0);
    await page.getByRole("button", { name: "Share" }).click();
    await expect(page.getByText("Link copied")).toBeVisible();
  });
}
