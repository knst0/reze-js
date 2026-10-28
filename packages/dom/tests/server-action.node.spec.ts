import { expect, test } from "vite-plus/test";

import * as s from "../../reze-js/src/schema";
import { handleServerAction, serverAction } from "../src/server-action";

const schema = s.object({ title: s.string(), count: s.optional(s.number()) });

test("a server action runs the handler with parsed input and answers JSON", async () => {
  const save = serverAction("test#json", schema, async (input) => `saved:${input.title}`);
  const response = await handleServerAction(
    new Request(`http://reze.invalid${save.endpoint}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ title: "one", count: 2 }),
    }),
  );
  expect(response?.status).toBe(200);
  expect(await response?.json()).toEqual({ ok: true, result: "saved:one" });
});

test("a server action rejects invalid input and unknown ids", async () => {
  const save = serverAction("test#invalid", schema, async (input) => input.title);
  const invalid = await handleServerAction(
    new Request(`http://reze.invalid${save.endpoint}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ title: 3 }),
    }),
  );
  expect(invalid?.status).toBe(422);
  expect(((await invalid?.json()) as { ok: boolean }).ok).toBe(false);
  const missing = await handleServerAction(
    new Request("http://reze.invalid/_reze/action/unknown", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: "{}",
    }),
  );
  expect(missing?.status).toBe(404);
});

test("a form post runs the action and redirects to its return path", async () => {
  serverAction("test#form", schema, async (input) => {
    expect(input.title).toBe("form");
    return "ok";
  });
  const response = await handleServerAction(
    new Request("http://reze.invalid/_reze/action/test%23form", {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded" },
      body: "title=form&_return=%2Fdone",
    }),
  );
  expect(response?.status).toBe(303);
  expect(response?.headers.get("location")).toBe("/done");
});

test("a form post that fails validation redirects with an error flag", async () => {
  serverAction("test#form-error", schema, async () => "ok");
  const response = await handleServerAction(
    new Request("http://reze.invalid/_reze/action/test%23form-error", {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded" },
      body: "_return=%2Fdone",
    }),
  );
  expect(response?.status).toBe(303);
  expect(response?.headers.get("location")).toBe("/done?_action_error=1");
});

test("a server action without a schema is refused before it can register", () => {
  expect(() => serverAction("test#missing", undefined as never, async () => 1)).toThrow(/input schema/);
});
