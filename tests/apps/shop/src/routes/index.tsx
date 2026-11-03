import { For, signal } from "reze-js";
import { mutation } from "reze-js/server";

import { addItems } from "../cart";
import { Icon, Stars } from "../parts";
import { products, type Product } from "../products";

export const addToCart = mutation(async (input: { id: number }, args) => addItems(1, args));

const categories = [...new Set(products.map((product) => product.category))].map((name) => ({
  name,
  count: products.filter((product) => product.category === name).length * 9,
}));

const prices = [
  { value: "0-50", label: "Under 50 dollars" },
  { value: "50-100", label: "50 to 100 dollars" },
  { value: "100-250", label: "100 to 250 dollars" },
  { value: "250-", label: "250 dollars and up" },
];

const pages = [1, 2, 3, 4, 5, 6, 7, 8, 9];

const scale = "M10 2v2.1l5 1.6 2.5 6.3a3 3 0 0 1-6 0L14 6.6 11 5.6V16h4v2H5v-2h4V5.6L6 6.6l2.5 5.4a3 3 0 0 1-6 0L5 5.7l5-1.6V2h0z";

function Card(props: { product: Product }) {
  let compared = signal(false);
  return (
    <li class="group relative flex flex-col overflow-hidden rounded-xl border border-stone-200 bg-white shadow-sm transition hover:shadow-md">
      <div class="aspect-[4/3] bg-stone-100">
        <img
          class="h-full w-full object-cover object-center transition group-hover:scale-105"
          src={`/images/${props.product.id}-640.jpg`}
          srcset={`/images/${props.product.id}-320.jpg 320w, /images/${props.product.id}-640.jpg 640w, /images/${props.product.id}-960.jpg 960w`}
          sizes="(min-width: 1024px) 25vw, (min-width: 640px) 50vw, 100vw"
          alt={props.product.name}
          width="640"
          height="480"
          loading="lazy"
        />
      </div>
      <div class="flex flex-1 flex-col gap-2 p-4">
        <p class="text-xs font-semibold uppercase tracking-wide text-emerald-800">{props.product.category}</p>
        <h2 class="text-base font-semibold text-stone-900">
          <a class="hover:underline" href={`/products/${props.product.id}`}>
            {props.product.name}
          </a>
        </h2>
        <p class="line-clamp-3 text-sm text-stone-600">{props.product.description}</p>
        <Stars rating={props.product.rating} reviews={props.product.reviews} />
        <p class="mt-auto text-lg font-bold text-stone-900">{props.product.price} dollars</p>
        <div class="flex items-center gap-2">
          <button
            class="flex-1 rounded-full bg-emerald-800 px-4 py-2 text-sm font-semibold text-white hover:bg-emerald-900 focus:outline-none focus-visible:ring-2 focus-visible:ring-emerald-600"
            onClick={() => addToCart.call({ id: props.product.id })}
          >
            Add {props.product.name} to cart
          </button>
          <button
            class="rounded-full border border-stone-300 p-2 text-stone-500 hover:text-emerald-800 aria-pressed:text-emerald-800"
            aria-pressed={compared}
            onClick={() => (compared = !compared)}
          >
            <Icon path={scale} label={`Compare ${props.product.name}`} />
          </button>
        </div>
      </div>
    </li>
  );
}

export default function Catalog() {
  return (
    <main class="mx-auto w-full max-w-7xl flex-1 px-4 py-8 sm:px-6 lg:px-8">
      <nav class="text-sm text-stone-500" aria-label="Breadcrumb">
        <a class="hover:text-emerald-800" href="/">
          Home
        </a>{" "}
        / All gear
      </nav>
      <h1 class="mt-2 text-3xl font-bold tracking-tight text-stone-900">Outdoor gear</h1>
      <p class="mt-2 max-w-2xl text-stone-600">
        Everything here has been carried on a real trip by someone on our staff. If it did not hold up, it is not on this page.
      </p>
      <div class="mt-8 grid gap-8 lg:grid-cols-[16rem_1fr]">
        <form class="space-y-6" action="/" aria-label="Filters">
          <fieldset>
            <legend class="text-sm font-semibold text-stone-900">Category</legend>
            <For each={categories}>
              {(category) => (
                <label class="mt-2 flex items-center gap-2 text-sm text-stone-700">
                  <input
                    class="h-4 w-4 rounded border-stone-300 text-emerald-800 focus:ring-emerald-600"
                    type="checkbox"
                    name="category"
                    value={category.name}
                  />
                  <span class="flex-1">{category.name}</span>
                  <span class="text-xs text-stone-500">{category.count}</span>
                </label>
              )}
            </For>
          </fieldset>
          <fieldset>
            <legend class="text-sm font-semibold text-stone-900">Price</legend>
            <For each={prices}>
              {(price) => (
                <label class="mt-2 flex items-center gap-2 text-sm text-stone-700">
                  <input
                    class="h-4 w-4 border-stone-300 text-emerald-800 focus:ring-emerald-600"
                    type="radio"
                    name="price"
                    value={price.value}
                  />
                  {price.label}
                </label>
              )}
            </For>
          </fieldset>
          <fieldset>
            <legend class="text-sm font-semibold text-stone-900">Customer rating</legend>
            <label class="mt-2 flex items-center gap-2 text-sm text-stone-700">
              <input
                class="h-4 w-4 rounded border-stone-300 text-emerald-800 focus:ring-emerald-600"
                type="checkbox"
                name="rating"
                value="4"
              />
              4 stars and up
            </label>
            <label class="mt-2 flex items-center gap-2 text-sm text-stone-700">
              <input
                class="h-4 w-4 rounded border-stone-300 text-emerald-800 focus:ring-emerald-600"
                type="checkbox"
                name="sale"
                value="1"
              />
              On sale
            </label>
            <label class="mt-2 flex items-center gap-2 text-sm text-stone-700">
              <input
                class="h-4 w-4 rounded border-stone-300 text-emerald-800 focus:ring-emerald-600"
                type="checkbox"
                name="stock"
                value="1"
              />
              In stock near me
            </label>
          </fieldset>
          <label class="block text-sm font-semibold text-stone-900">
            Sort by
            <select class="mt-2 block w-full rounded-lg border-stone-300 text-sm font-normal" name="sort">
              <option value="featured">Featured</option>
              <option value="rating">Highest rated</option>
              <option value="price">Price, low to high</option>
              <option value="-price">Price, high to low</option>
              <option value="new">Newest</option>
            </select>
          </label>
          <button
            class="w-full rounded-full border border-emerald-800 px-4 py-2 text-sm font-semibold text-emerald-800 hover:bg-emerald-50"
            type="submit"
          >
            Apply filters
          </button>
        </form>
        <div>
          <p class="text-sm text-stone-600">Showing 1 to 24 of 212 products</p>
          <ul class="mt-4 grid grid-cols-1 gap-6 sm:grid-cols-2 xl:grid-cols-3" aria-label="Products">
            <For each={products} key="id">
              {(product) => <Card product={product} />}
            </For>
          </ul>
          <nav class="mt-10 flex items-center justify-center gap-2 text-sm" aria-label="Pages">
            <For each={pages}>
              {(page) => (
                <a
                  class="rounded-lg border border-stone-300 px-3 py-2 text-stone-700 hover:border-emerald-700 hover:text-emerald-800"
                  href={`/?page=${page}`}
                >
                  {page}
                </a>
              )}
            </For>
          </nav>
        </div>
      </div>
    </main>
  );
}
