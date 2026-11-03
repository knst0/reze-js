import type { MutationArgs } from "reze-js/server";

export function addItems(quantity: number, { context, response }: MutationArgs): void {
  context.cart += quantity;
  response.cookies.set("cart", String(context.cart), { secure: false });
}
