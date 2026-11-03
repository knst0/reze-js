import type { Page } from "@playwright/test";

import { expect, test, type Server } from "../fixtures";

let users = 0;

async function signIn(page: Page, server: Server): Promise<void> {
  await page.goto(server.url("/login"));
  await page.getByLabel("Name").fill(`nav${process.pid}x${users++}x${Date.now().toString(36)}`);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("heading", { name: "Todos" })).toBeVisible();
}

async function add(page: Page, title: string, priority = "2"): Promise<void> {
  await page.getByLabel("Title").fill(title);
  await page.getByLabel("Priority").fill(priority);
  await page.getByRole("button", { name: "Add" }).click();
}

/** Marks the live document and the layout's `<nav>`, so a test sees whether a full load replaced them. */
async function mark(page: Page): Promise<void> {
  await page.evaluate(() => {
    (window as unknown as { kept: boolean }).kept = true;
    (document.querySelector("nav") as unknown as { kept: boolean }).kept = true;
  });
}

const kept = (page: Page) =>
  page.evaluate(() => [
    (window as unknown as { kept?: boolean }).kept,
    (document.querySelector("nav") as unknown as { kept?: boolean }).kept,
  ]);

test("navigates without a document load and keeps layout DOM", async ({ page, server }) => {
  await signIn(page, server);
  await add(page, "Read a book");
  await expect(page.getByRole("link", { name: "Read a book" })).toBeVisible();
  await mark(page);
  const segments: string[] = [];
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (url.searchParams.has("_b")) segments.push(url.searchParams.get("_s") ?? "0");
  });
  await page.getByRole("link", { name: "Read a book" }).click();
  await expect(page).toHaveURL(server.url("/todos/1"));
  await expect(page.getByRole("heading", { name: "Read a book" })).toBeFocused();
  expect(await kept(page)).toEqual([true, true]);
  expect(segments).toContain("2");

  await page.goBack();
  await expect(page).toHaveURL(server.url("/todos"));
  await expect(page.getByRole("link", { name: "Read a book" })).toBeVisible();
  await page.goForward();
  await expect(page.getByRole("heading", { name: "Read a book" })).toBeVisible();
  await page.getByRole("link", { name: "Todos" }).click();
  await expect(page.getByRole("heading", { name: "Todos" })).toBeVisible();
  expect(await kept(page)).toEqual([true, true]);
});

test("prefetches a link on intent", async ({ page, server }) => {
  await signIn(page, server);
  await add(page, "Prefetch me");
  const link = page.getByRole("link", { name: "Prefetch me" });
  await expect(link).toBeVisible();
  const prefetch = page.waitForRequest((request) => request.headers().purpose === "prefetch");
  await link.hover();
  const request = await prefetch;
  expect(new URL(request.url()).searchParams.get("_s")).toBe("2");
  let fetched = 0;
  page.on("request", (request) => {
    if (new URL(request.url()).pathname === "/todos/1") fetched++;
  });
  await link.click();
  await expect(page.getByRole("heading", { name: "Prefetch me" })).toBeVisible();
  expect(fetched).toBe(0);
});

test("applies mutations in place", async ({ page, server }) => {
  await signIn(page, server);
  await mark(page);
  const kinds: string[] = [];
  await page.exposeFunction("swapped", (kind: string) => kinds.push(kind));
  await page.evaluate(() =>
    addEventListener("rz:swap", (event) =>
      (window as unknown as { swapped(kind: string): void }).swapped((event as CustomEvent).detail.kind),
    ),
  );
  await add(page, "Walk the dog", "7");
  await expect(page.getByRole("alert")).toContainText("Priority:");
  await expect(page.getByLabel("Title")).toHaveValue("Walk the dog");
  await page.getByLabel("Priority").fill("3");
  await page.getByRole("button", { name: "Add" }).click();
  await expect(page.getByRole("list", { name: "Todos" }).getByRole("listitem")).toHaveText([/Walk the dog P3 open/]);
  await expect(page.getByRole("alert")).toHaveCount(0);
  expect(await kept(page)).toEqual([true, true]);
  expect(kinds).toEqual(["mutation", "mutation"]);
});

test("falls back to a document load when the build differs", async ({ page, server }) => {
  await signIn(page, server);
  await add(page, "Stale");
  await mark(page);
  await page.route(/_b=/, (route) => route.fulfill({ status: 409, body: "Conflict" }));
  await page.getByRole("link", { name: "Stale" }).click();
  await expect(page.getByRole("heading", { name: "Stale" })).toBeVisible();
  expect(await kept(page)).toEqual([undefined, undefined]);
});

test("answers a stale build with 409", async ({ request, server }) => {
  const response = await request.get(server.url("/login?_b=stale"));
  expect(response.status()).toBe(409);
});
