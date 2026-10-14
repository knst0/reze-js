import { createQueryClient } from "@rezejs/query";

const client = createQueryClient();

export const loadUser = (id: number) => client.fetchQuery({ queryKey: ["user", id], queryFn: async () => ({ id }) });
