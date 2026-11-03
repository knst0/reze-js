import { get } from "node:http";

import type { Page } from "@playwright/test";

import { expect, test, type Server } from "../fixtures";

let users = 0;

async function signIn(page: Page, server: Server): Promise<string> {
  const name = `user${process.pid}x${users++}x${Date.now().toString(36)}`;
  await page.goto(server.url("/todos"));
  await expect(page).toHaveURL(server.url("/login"));
  await page.getByLabel("Name").fill(name);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByText(`Signed in as ${name}`)).toBeVisible();
  return name;
}

async function add(page: Page, title: string, priority = "2"): Promise<void> {
  await page.getByLabel("Title").fill(title);
  await page.getByLabel("Priority").fill(priority);
  await page.getByRole("button", { name: "Add" }).click();
}

for (const javaScriptEnabled of [true, false]) {
  test.describe(javaScriptEnabled ? "with JS" : "without JS", () => {
    test.use({ javaScriptEnabled });

    test("creates and toggles todos", async ({ page, server }) => {
      await signIn(page, server);
      await add(page, "Buy milk");
      const todos = page.getByRole("list", { name: "Todos" });
      await expect(todos.getByRole("listitem")).toHaveText([/Buy milk P2 open/]);
      await expect(page).toHaveURL(server.url("/todos"));
      await expect(todos.locator('form[method="post"][action*="_m="] > button:not([data-rz])')).toHaveText("Toggle Buy milk");
      await page.getByRole("button", { name: "Toggle Buy milk" }).click();
      await expect(todos.getByRole("listitem")).toHaveText([/Buy milk P2 done/]);
      await page.goto(server.url("/todos?filter=open"));
      await expect(todos.getByRole("listitem")).toHaveCount(0);
    });

    test("reports invalid input and keeps it", async ({ page, server }) => {
      await signIn(page, server);
      const response = page.waitForResponse((response) => response.request().method() === "POST");
      await add(page, "Walk the dog", "7");
      expect((await response).status()).toBe(422);
      await expect(page.getByRole("alert")).toContainText("Priority:");
      await expect(page.getByLabel("Title")).toHaveValue("Walk the dog");
      await expect(page.getByRole("list", { name: "Todos" }).getByRole("listitem")).toHaveCount(0);
    });
  });
}

test("guards the app behind sign-in", async ({ request, server }) => {
  const response = await request.get(server.url("/todos"), { maxRedirects: 0 });
  expect(response.status()).toBe(307);
  expect(response.headers().location).toBe("/login");
});

test("renders route errors in the boundary", async ({ page, server, serverKind }) => {
  await signIn(page, server);
  let response = await page.goto(server.url("/todos/1"));
  expect(response!.status()).toBe(404);
  await expect(page.getByRole("alert")).toHaveText("404 Not Found");
  await expect(page.getByRole("link", { name: "Todos" })).toBeVisible();
  response = await page.goto(server.url("/todos/abc"));
  expect(response!.status()).toBe(404);
  response = await page.goto(server.url("/nowhere"));
  expect(response!.status()).toBe(404);
  await expect(page.getByRole("alert")).toHaveText("404 Not Found");
  response = await page.goto(server.url("/broken"));
  expect(response!.status()).toBe(500);
  await expect(page.getByRole("alert")).toContainText("500");
  if (serverKind !== "js") await expect(page.getByRole("alert")).toHaveText("500 Internal Server Error");
});

test("answers with typed params, headers, and cookies", async ({ page, server }) => {
  await signIn(page, server);
  await add(page, "Read a book");
  await expect(page.getByRole("link", { name: "Read a book" })).toBeVisible();
  const response = await page.goto(server.url("/todos/1"));
  await expect(page.getByRole("heading", { name: "Read a book" })).toBeVisible();
  expect(response!.headers()["cache-control"]).toBe("private, max-age=60");
  expect(await response!.headerValue("set-cookie")).toContain("last=1");
});

test("renders only the segments a mutation revalidates", async ({ page, server }) => {
  const name = await signIn(page, server);
  await add(page, "Water plants");
  await expect(page.getByRole("button", { name: "Toggle Water plants" })).toBeVisible();
  const layout = page.getByText(`Signed in as ${name}`);
  await layout.evaluate((node) => node.setAttribute("data-kept", ""));
  const response = page.waitForResponse((response) => response.request().method() === "POST");
  await page.getByRole("button", { name: "Toggle Water plants" }).click();
  expect((await response).headers()["rz-segment"]).toBe("2");
  await expect(page.getByRole("list", { name: "Todos" }).getByRole("listitem")).toHaveText([/Water plants P2 done/]);
  await expect(layout).toHaveAttribute("data-kept", "");
  await add(page, "Feed the cat");
  await expect(page.getByRole("list", { name: "Todos" }).getByRole("listitem")).toHaveCount(2);
  await expect(layout).not.toHaveAttribute("data-kept");
});

test("calls a mutation from a handler and morphs the page", async ({ page, server }) => {
  await signIn(page, server);
  await add(page, "Read a book");
  await page.getByRole("link", { name: "Read a book" }).click();
  await expect(page.getByRole("heading", { name: "Read a book" })).toBeVisible();
  await page.getByLabel("New title").fill("Read two books");
  await page.getByLabel("New priority").fill("3");
  await page.getByRole("button", { name: "Rename" }).click();
  await expect(page.getByRole("heading", { name: "Read two books" })).toBeVisible();
  await expect(page.getByText("Priority 3")).toBeVisible();
  await expect(page.getByLabel("New title")).toHaveValue("Read two books");
  await page.getByLabel("New priority").fill("7");
  await page.getByRole("button", { name: "Rename" }).click();
  await expect(page.getByRole("status")).toHaveText("Invalid priority");
  await expect(page.getByText("Priority 3")).toBeVisible();
});

test("writes Server-Timing in development only", async ({ request, server, serverKind }) => {
  const timing = (await request.get(server.url("/about"))).headers()["server-timing"];
  if (serverKind === "js") expect(timing).toMatch(/^loader-1;dur=\d+\.\d, render;dur=\d+\.\d$/);
  else expect(timing).toBeUndefined();
});

test("redirects to the canonical URL", async ({ request, server }) => {
  const response = await request.get(server.url("/Login/"), { maxRedirects: 0 });
  expect(response.status()).toBe(308);
  expect(response.headers().location).toBe("/login");
});

test("rejects cross-site posts", async ({ page, request, server }) => {
  const user = await signIn(page, server);
  const action = await page.locator("form", { has: page.getByRole("button", { name: "Add" }) }).getAttribute("action");
  const form = { title: "Forged", priority: "1" };
  for (const headers of [{ "sec-fetch-site": "cross-site" }, { origin: "https://evil.example" }] as Record<string, string>[]) {
    const response = await request.post(server.url(action!), { headers: { ...headers, cookie: `user=${user}` }, form, maxRedirects: 0 });
    expect(response.status()).toBe(403);
  }
});

test("accepts a JSON call", async ({ page, request, server }) => {
  const user = await signIn(page, server);
  const action = await page.locator("form", { has: page.getByRole("button", { name: "Add" }) }).getAttribute("action");
  const headers = { cookie: `user=${user}` };
  let response = await request.post(server.url(action!), { headers, data: { title: "From JSON", priority: 3 } });
  expect(response.status()).toBe(200);
  expect(await response.json()).toEqual({ result: { id: 1, title: "From JSON", priority: 3, done: false } });
  response = await request.post(server.url(action!), { headers, data: { title: "Bad", priority: "3" } });
  expect(response.status()).toBe(422);
  expect((await response.json()).issues[0].path).toEqual(["priority"]);
});

test("sends the dispatcher in early hints before the loaders finish", async ({ server }) => {
  const hints = await new Promise<string[]>((done, fail) => {
    const links: string[] = [];
    get(server.url("/about?from=hints"), (response) => {
      response.resume();
      response.on("end", () => done(links));
    })
      .on("information", (info) => {
        if (info.statusCode === 103) links.push(String(info.headers.link));
      })
      .on("error", fail);
  });
  expect(hints).toEqual([expect.stringMatching(/^<\/.*dispatcher.*>; rel=modulepreload$/)]);
});
