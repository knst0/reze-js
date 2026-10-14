import { createQueryClient } from "@rezejs/query";

export const restore = (state: Parameters<ReturnType<typeof createQueryClient>["hydrate"]>[0]) => {
  const client = createQueryClient();
  client.hydrate(state);
  return client.dehydrate();
};
