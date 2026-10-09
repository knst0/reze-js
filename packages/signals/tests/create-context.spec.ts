import { expect, test } from "vite-plus/test";

import { ContextNotFoundError, createContext, provideContext, root, useContext } from "../src";

test("a default applies outside a provider", () => {
  const Theme = createContext("light");
  expect(useContext(Theme)).toBe("light");
  root(() => expect(useContext(Theme)).toBe("light"));
});

test("the component shadows the default and the nearest wins", () => {
  const Theme = createContext("light");
  const seen: string[] = [];
  root(() => {
    Theme({
      value: "dark",
      get children() {
        seen.push(useContext(Theme));
        Theme({
          value: "blue",
          get children() {
            seen.push(useContext(Theme));
            return null;
          },
        });
        seen.push(useContext(Theme));
        return null;
      },
    });
  });
  expect(seen).toEqual(["dark", "blue", "dark"]);
});

test("a default-less context throws outside a provider and reads inside", () => {
  const Ctx = createContext<string>();
  expect(() => useContext(Ctx)).toThrow(ContextNotFoundError);
  root(() => expect(() => useContext(Ctx)).toThrow(ContextNotFoundError));
  root(() => {
    provideContext(Ctx, "v", () => expect(useContext(Ctx)).toBe("v"));
    Ctx({
      value: "w",
      get children() {
        expect(useContext(Ctx)).toBe("w");
        return null;
      },
    });
  });
  try {
    useContext(Ctx);
  } catch (error) {
    expect(error).toBeInstanceOf(Error);
    expect((error as Error).name).toBe("ContextNotFoundError");
    return;
  }
  expect.unreachable();
});

test("an explicit undefined default is a default, not a missing one", () => {
  const Maybe = createContext<string | undefined>(undefined);
  expect(useContext(Maybe)).toBeUndefined();
  root(() => expect(useContext(Maybe)).toBeUndefined());
});

test("the value is read once, so containers stay stable", () => {
  const Theme = createContext("light");
  let reads = 0;
  root(() => {
    Theme({
      get value() {
        reads++;
        return "dark";
      },
      get children() {
        expect(useContext(Theme)).toBe("dark");
        return null;
      },
    });
  });
  expect(reads).toBe(1);
});
