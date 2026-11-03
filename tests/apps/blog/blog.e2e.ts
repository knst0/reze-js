import type { APIRequestContext } from "@playwright/test";

import { expect, test } from "../fixtures";

const paths = ["/", "/?ref=mail&_s=x", "/posts/1?ref=feed", "/posts/1", "/posts/2", "/posts/9", "/posts/abc", "/posts/members", "/missing"];

test("renders the same HTML on the Rust and the JS server", async ({ request, server }) => {
  const headers = { "rz-prerender": "1" };
  for (const path of paths) {
    const [rust, js] = await Promise.all([request.get(server.url(path), { headers }), request.get(server.js!(path), { headers })]);
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

/** The URL a page's comment form posts to. */
async function action(request: APIRequestContext, url: string) {
  const html = await (await request.get(url)).text();
  return new URL(/<form method="post" action="([^"]+)"/.exec(html)![1]!.replaceAll("&amp;", "&"), url).href;
}

const servers = (server: { url: (path: string) => string; js?: (path: string) => string }) => [
  (path: string) => server.url(path),
  (path: string) => server.js!(path),
];

test("runs a Rust mutation from a form post", async ({ request, server }) => {
  for (const url of servers(server)) {
    const target = await action(request, url("/posts/2?ref=feed"));
    const author = `Reader ${Math.random()}`;
    const posted = await request.post(target, { form: { post: "2", author, body: "Nice", stars: "2" }, maxRedirects: 0 });
    expect(posted.status()).toBe(303);
    expect(posted.headers().location).toBe("/posts/2?ref=feed");
    expect(await (await request.get(url("/posts/2"))).text()).toContain(`${author}: Nice**`);
  }
});

test("renders the same submission and mutation errors on the Rust and the JS server", async ({ request, server }) => {
  const forms = [
    { post: "1", author: "Ann", body: "Hi", stars: "lots" },
    { post: "1", author: "Ann", body: "Hi", stars: "300" },
    { post: "1", author: "", body: "Hi", stars: "" },
  ];
  for (const form of forms) {
    const [rust, js] = await Promise.all(
      servers(server).map(async (url) => request.post(await action(request, url("/posts/1")), { form })),
    );
    expect(rust!.status(), JSON.stringify(form)).toBe(js!.status());
    expect(await rust!.text(), JSON.stringify(form)).toBe(await js!.text());
  }
});

test("redirects from a Rust mutation with 303", async ({ request, server }) => {
  for (const url of servers(server)) {
    const target = await action(request, url("/posts/1"));
    const posted = await request.post(target, { form: { post: "1", author: "Ann", body: "home" }, maxRedirects: 0 });
    expect(posted.status()).toBe(303);
    expect(posted.headers().location).toBe("/");
  }
});

test("answers a Rust mutation's call as JSON", async ({ request, server }) => {
  for (const url of servers(server)) {
    const target = await action(request, url("/posts/1"));
    const called = await request.post(target, { data: { post: 1, author: "Bot", body: "Called", stars: null } });
    expect(called.status()).toBe(200);
    expect(((await called.json()) as { result: { count: number } }).result.count).toBeGreaterThan(0);
    const invalid = await request.post(target, { data: { post: "1", author: "Bot" } });
    expect(invalid.status()).toBe(422);
    expect(await invalid.json()).toEqual({
      issues: [
        { message: "Expected an integer from 0 to 4294967295", path: ["post"] },
        { message: "Expected a string", path: ["body"] },
        { message: "Expected an integer from 0 to 255 or null", path: ["stars"] },
      ],
    });
    const failed = await request.post(target, { data: { post: 1, author: "", body: "Called", stars: null } });
    expect(failed.status()).toBe(400);
    expect(await failed.json()).toEqual({ message: "Comments need an author" });
  }
});

test("rejects a cross-site post to a Rust mutation", async ({ request, server }) => {
  const target = await action(request, server.url("/posts/1"));
  const posted = await request.post(target, {
    form: { post: "1", author: "Eve", body: "Hi" },
    headers: { "sec-fetch-site": "cross-site" },
  });
  expect(posted.status()).toBe(403);
});

test("writes Server-Timing with the dev feature", async ({ request, server }) => {
  const timing = (await request.get(server.url("/posts/1"))).headers()["server-timing"];
  expect(timing).toMatch(/^loader-1;dur=\d+\.\d, render;dur=\d+\.\d$/);
});

test.describe("without JS", () => {
  test.use({ javaScriptEnabled: false });

  test("posts a comment and shows the issues of invalid input", async ({ page, server }) => {
    await page.goto(server.url("/posts/1"));
    const author = `Reader ${Math.random()}`;
    await page.getByLabel("Author").fill(author);
    await page.getByRole("textbox", { name: "Comment" }).fill("Plain");
    await page.getByLabel("Stars").fill("many");
    await page.getByRole("button", { name: "Comment" }).click();
    await expect(page.getByRole("alert")).toHaveText("Stars: Expected an integer from 0 to 255");
    await expect(page.getByLabel("Author")).toHaveValue(author);
    await page.getByLabel("Stars").fill("1");
    await page.getByRole("button", { name: "Comment" }).click();
    await expect(page).toHaveURL(server.url("/posts/1"));
    await expect(page.getByRole("region", { name: "Comments" })).toContainText(`${author}: Plain*`);
  });

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

test("posts a comment through the client and revalidates the page", async ({ page, server }) => {
  await page.goto(server.url("/posts/1"));
  const author = `Reader ${Math.random()}`;
  await page.getByLabel("Author").fill(author);
  await page.getByRole("textbox", { name: "Comment" }).fill("Live");
  await page.getByRole("button", { name: "Comment" }).click();
  await expect(page.getByRole("region", { name: "Comments" })).toContainText(`${author}: Live`);
  await page.getByLabel("Stars").fill("x");
  await page.getByRole("button", { name: "Comment" }).click();
  await expect(page.getByRole("alert")).toHaveText("Stars: Expected an integer from 0 to 255");
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

test("streams deferred Rust fields into their boundaries", async ({ page, server }) => {
  const response = page.waitForResponse(server.url("/"));
  await page.goto(server.url("/"), { waitUntil: "commit" });
  await expect(page.getByRole("heading", { name: "Blog" })).toBeVisible();
  expect((await response).headers().etag).toBeUndefined();
  await expect(page.getByRole("button", { name: "Views 42" })).toBeVisible();
  await expect(page.getByRole("list", { name: "Topics" }).getByRole("listitem")).toHaveText(["intro", "rust"]);
  await expect(page.getByText("Loading topics…")).toHaveCount(0);
  await expect(page.locator("template")).toHaveCount(0);
  await page.getByRole("button", { name: "Views 42" }).click();
  await expect(page.getByRole("button", { name: "Views 43" })).toBeVisible();
});

test("writes each deferred part after the document in resolution order", async ({ request, server }) => {
  const html = await (await request.get(server.url("/"))).text();
  expect(html).toContain("<!--l0--><p>Counting views…</p><!--/l-->");
  expect(html).toContain("<!--l1--><p>Loading topics…</p><!--/l-->");
  expect(html.indexOf('<template data-rz-s="0">')).toBeLessThan(html.indexOf('<template data-rz-s="1">'));
  expect(html).toMatch(/<template data-rz-s="1"><ul aria-label=Topics><li>intro<\/li><li>rust<\/li><\/ul><\/template>\n<\/body><\/html>$/);
});

test("streams deferred Rust fields after a client navigation", async ({ page, server }) => {
  await page.goto(server.url("/posts/1"));
  await page.getByRole("link", { name: "Home" }).click();
  await expect(page.getByRole("list", { name: "Topics" }).getByRole("listitem")).toHaveText(["intro", "rust"]);
  await page.getByRole("button", { name: "Views 42" }).click();
  await expect(page.getByRole("button", { name: "Views 43" })).toBeVisible();
});

test("keeps deferred Rust parts in templates without JS", async ({ browser, server }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto(server.url("/"));
  await expect(page.getByText("Loading topics…")).toBeVisible();
  await expect(page.getByRole("list", { name: "Topics" })).toHaveCount(0);
  await context.close();
});
