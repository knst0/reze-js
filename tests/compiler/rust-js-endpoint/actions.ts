import { mutation } from "reze-js/server";

export const subscribe = mutation(async (input: { email: string }) => input.email);
