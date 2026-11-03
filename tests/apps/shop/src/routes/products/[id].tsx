import { For, signal, useParams } from "reze-js";
import { mutation } from "reze-js/server";

import { addItems } from "../../cart";
import { Stars } from "../../parts";
import { products } from "../../products";
import { reviews } from "../../reviews";

export const addQuantity = mutation(async (input: { id: number; quantity: number }, args) => addItems(input.quantity, args));

const views = ["front", "side", "top", "detail"];

const related = products.slice(0, 4);

const questions = [
  {
    question: "Does it work with a hydration bladder?",
    answer: "Yes. Cut the drinking tube and fit the adapter that comes in the box on both ends, then the filter sits inline.",
  },
  {
    question: "How long does the filter last?",
    answer:
      "The maker rates it for 100,000 gallons. In practice it is retired when backflushing no longer restores the flow, or after it freezes.",
  },
  {
    question: "Does it remove viruses?",
    answer: "No. It removes bacteria and protozoa. Where viruses are a concern, also treat the water with tablets or boil it.",
  },
  {
    question: "Can I take it on a plane?",
    answer: "Yes, empty and dry, in carry-on or checked bags. Let it dry for a few days at home first so nothing grows inside.",
  },
];

export default function ProductPage() {
  const params = useParams();
  const product = products.find((product) => String(product.id) === params.id)!;
  let view = signal("front");
  let quantity = signal(1);
  return (
    <main class="mx-auto w-full max-w-7xl flex-1 px-4 py-8 sm:px-6 lg:px-8">
      <head>
        <title>{product.name}</title>
        <meta name="description" content={product.description} />
      </head>
      <nav class="text-sm text-stone-500" aria-label="Breadcrumb">
        <a class="hover:text-emerald-800" href="/">
          Home
        </a>{" "}
        /{" "}
        <a class="hover:text-emerald-800" href={`/?category=${product.category}`}>
          {product.category}
        </a>{" "}
        / {product.name}
      </nav>
      <div class="mt-6 grid gap-10 lg:grid-cols-2">
        <section aria-label="Gallery">
          <figure class="overflow-hidden rounded-xl border border-stone-200 bg-stone-100">
            <img
              class="aspect-[4/3] h-full w-full object-cover object-center"
              src={`/images/${product.id}-${view}.jpg`}
              alt={`${product.name}, ${view} view`}
              width="640"
              height="480"
            />
            <figcaption class="px-4 py-2 text-xs text-stone-500">Photographed on the trail by our staff.</figcaption>
          </figure>
          <ul class="mt-4 grid grid-cols-4 gap-3" aria-label="Views">
            <For each={views}>
              {(name) => (
                <li>
                  <button
                    class="w-full rounded-lg border border-stone-300 px-2 py-2 text-sm capitalize text-stone-700 hover:border-emerald-700 aria-pressed:border-emerald-800 aria-pressed:bg-emerald-50"
                    aria-pressed={view === name}
                    onClick={() => (view = name)}
                  >
                    {name}
                  </button>
                </li>
              )}
            </For>
          </ul>
        </section>
        <section class="flex flex-col gap-4" aria-label="Purchase">
          <p class="text-xs font-semibold uppercase tracking-wide text-emerald-800">{product.category}</p>
          <h1 class="text-3xl font-bold tracking-tight text-stone-900">{product.name}</h1>
          <Stars rating={product.rating} reviews={product.reviews} />
          <p class="text-2xl font-bold text-stone-900">{product.price} dollars</p>
          <p class="text-sm text-stone-600">Or four payments of a quarter of that with no interest, at checkout.</p>
          <div class="flex items-center gap-3">
            <div class="flex items-center rounded-full border border-stone-300">
              <button
                class="px-3 py-2 text-lg text-stone-700 hover:text-emerald-800"
                aria-label="Fewer"
                onClick={() => quantity > 1 && quantity--}
              >
                -
              </button>
              <output class="w-8 text-center text-sm font-semibold" aria-label="Quantity">
                {quantity}
              </output>
              <button class="px-3 py-2 text-lg text-stone-700 hover:text-emerald-800" aria-label="More" onClick={() => quantity++}>
                +
              </button>
            </div>
            <button
              class="flex-1 rounded-full bg-emerald-800 px-6 py-3 text-sm font-semibold text-white hover:bg-emerald-900 focus:outline-none focus-visible:ring-2 focus-visible:ring-emerald-600"
              onClick={() => addQuantity.call({ id: product.id, quantity })}
            >
              Add to cart
            </button>
          </div>
          <ul class="space-y-1 text-sm text-stone-600">
            <li>In stock at the warehouse, and at 12 stores near you.</li>
            <li>Orders placed before two in the afternoon ship the same day and arrive in two to four business days.</li>
            <li>Free returns for 60 days, worn or not.</li>
          </ul>
        </section>
      </div>
      <section class="mt-12 max-w-3xl space-y-4 text-stone-700" aria-label="Description">
        <h2 class="text-xl font-bold text-stone-900">Description</h2>
        <p>{product.description}</p>
        <p>
          We test every product on a staff trip of at least a week before it goes on sale. We look at how it holds up to rain, grit, cold
          mornings, and being stuffed into the bottom of a pack, and we write down what breaks. If something fails in a way the maker cannot
          explain, we do not sell it.
        </p>
        <p>
          Every purchase is covered by our sixty day return policy, worn or not, and by the manufacturer's warranty after that. Members can
          bring gear to any of our stores for a free inspection and basic repairs, and we recycle what cannot be fixed.
        </p>
      </section>
      <section class="mt-12 max-w-3xl" aria-label="Specifications">
        <h2 class="text-xl font-bold text-stone-900">Specifications</h2>
        <table class="mt-4 w-full border-collapse text-left text-sm">
          <tbody class="divide-y divide-stone-200">
            <tr>
              <th class="w-1/3 py-2 pr-4 font-medium text-stone-900">Category</th>
              <td class="py-2 text-stone-700">{product.category}</td>
            </tr>
            <tr>
              <th class="w-1/3 py-2 pr-4 font-medium text-stone-900">Price</th>
              <td class="py-2 text-stone-700">{product.price} dollars</td>
            </tr>
            <tr>
              <th class="w-1/3 py-2 pr-4 font-medium text-stone-900">Warranty</th>
              <td class="py-2 text-stone-700">Two years, parts and labor</td>
            </tr>
            <tr>
              <th class="w-1/3 py-2 pr-4 font-medium text-stone-900">Made in</th>
              <td class="py-2 text-stone-700">Assembled in Portland, Oregon, from imported materials</td>
            </tr>
            <tr>
              <th class="w-1/3 py-2 pr-4 font-medium text-stone-900">Care</th>
              <td class="py-2 text-stone-700">Rinse with clean water after use and dry fully before storage</td>
            </tr>
            <tr>
              <th class="w-1/3 py-2 pr-4 font-medium text-stone-900">Item number</th>
              <td class="py-2 text-stone-700">SO-{product.id}</td>
            </tr>
          </tbody>
        </table>
      </section>
      <section class="mt-12 max-w-3xl" aria-label="Questions">
        <h2 class="text-xl font-bold text-stone-900">Questions and answers</h2>
        <For each={questions}>
          {(question) => (
            <div class="border-b border-stone-200 py-4">
              <h3 class="font-semibold text-stone-900">{question.question}</h3>
              <p class="mt-2 text-stone-700">{question.answer}</p>
            </div>
          )}
        </For>
      </section>
      <section class="mt-12" aria-label="Also bought">
        <h2 class="text-xl font-bold text-stone-900">Customers also bought</h2>
        <ul class="mt-4 grid grid-cols-2 gap-6 lg:grid-cols-4">
          <For each={related}>
            {(item) => (
              <li class="rounded-xl border border-stone-200 p-4">
                <img
                  class="aspect-[4/3] w-full rounded-lg object-cover"
                  src={`/images/${item.id}-320.jpg`}
                  alt={item.name}
                  width="320"
                  height="240"
                  loading="lazy"
                />
                <a class="mt-3 block font-semibold text-stone-900 hover:underline" href={`/products/${item.id}`}>
                  {item.name}
                </a>
                <p class="text-sm text-stone-600">{item.price} dollars</p>
              </li>
            )}
          </For>
        </ul>
      </section>
      <section class="mt-12 max-w-3xl" aria-label="Reviews">
        <h2 class="text-xl font-bold text-stone-900">Reviews</h2>
        <For each={reviews}>
          {(review) => (
            <article class="border-b border-stone-200 py-6">
              <h3 class="font-semibold text-stone-900">{review.title}</h3>
              <p class="mt-1 text-sm text-stone-500">
                {review.rating} stars by {review.author} on {review.date}
              </p>
              <p class="mt-3 text-stone-700">{review.body}</p>
            </article>
          )}
        </For>
      </section>
    </main>
  );
}
