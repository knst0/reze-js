import { loader } from "reze-js/server";

import { Footer, Header } from "./parts";

export const load = loader(async ({ context }) => ({ cart: context.cart }));

export default function Root(props: { data: { cart: number }; children: unknown }) {
  return (
    <div class="flex min-h-screen flex-col bg-white font-sans text-stone-900 antialiased">
      <head>
        <title>Summit Outfitters</title>
        <meta name="description" content="Gear for hiking, camping, and backpacking, chosen and tested by people who use it." />
        <link rel="stylesheet" href="/styles.css" />
      </head>
      <Header cart={props.data.cart} />
      {props.children}
      <Footer />
    </div>
  );
}
