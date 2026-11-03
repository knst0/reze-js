import { For } from "reze-js";

import { departments, help } from "./departments";

export function Icon(props: { path: string; label: string }) {
  return (
    <svg class="h-5 w-5 flex-none fill-current" viewBox="0 0 20 20" role="img" aria-label={props.label}>
      <path d={props.path} />
    </svg>
  );
}

export const star = "M10 1.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L10 14.9l-5.2 2.7 1-5.8L1.5 7.7l5.9-.9L10 1.5z";
const bag = "M6 6V5a4 4 0 1 1 8 0v1h3l-1 12H4L3 6h3zm2 0h4V5a2 2 0 1 0-4 0v1z";
const search = "M8.5 3a5.5 5.5 0 0 1 4.4 8.8l4 4-1.4 1.4-4-4A5.5 5.5 0 1 1 8.5 3zm0 2a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7z";
const user = "M10 10a4 4 0 1 0 0-8 4 4 0 0 0 0 8zm-7 8a7 7 0 0 1 14 0H3z";

export function Stars(props: { rating: number; reviews: number }) {
  return (
    <p class="mt-1 flex items-center gap-1 text-sm text-stone-600">
      <span class="flex text-amber-500" aria-hidden="true">
        <Icon path={star} label="star" />
        <Icon path={star} label="star" />
        <Icon path={star} label="star" />
        <Icon path={star} label="star" />
        <Icon path={star} label="star" />
      </span>
      <span>
        {props.rating} out of 5, {props.reviews} reviews
      </span>
    </p>
  );
}

export function Header(props: { cart: number }) {
  return (
    <header class="sticky top-0 z-20 border-b border-stone-200 bg-white/95 backdrop-blur">
      <p class="bg-emerald-900 px-4 py-2 text-center text-sm font-medium text-emerald-50">
        Members save 10 percent on one full-price item through Sunday.{" "}
        <a class="underline underline-offset-2" href="/membership">
          Join for 30 dollars, once
        </a>
      </p>
      <div class="mx-auto flex max-w-7xl items-center gap-6 px-4 py-3 sm:px-6 lg:px-8">
        <a class="text-xl font-bold tracking-tight text-emerald-900" href="/">
          Summit Outfitters
        </a>
        <form class="relative hidden flex-1 md:block" action="/search" role="search">
          <label class="sr-only" for="q">
            Search gear
          </label>
          <input
            class="w-full rounded-full border border-stone-300 bg-stone-50 py-2 pl-10 pr-4 text-sm focus:border-emerald-700 focus:outline-none"
            id="q"
            name="q"
            type="search"
            placeholder="Search tents, boots, stoves"
          />
          <span class="pointer-events-none absolute left-3 top-2.5 text-stone-400">
            <Icon path={search} label="Search" />
          </span>
        </form>
        <a class="flex items-center gap-2 text-sm font-medium text-stone-700 hover:text-emerald-800" href="/account">
          <Icon path={user} label="Account" />
          <span class="hidden lg:inline">Sign in</span>
        </a>
        <a class="flex items-center gap-2 text-sm font-medium text-stone-700 hover:text-emerald-800" href="/cart">
          <Icon path={bag} label="Bag" />
          <span aria-label="Cart">{props.cart} in cart</span>
        </a>
      </div>
      <nav class="mx-auto hidden max-w-7xl px-4 sm:px-6 lg:block lg:px-8" aria-label="Departments">
        <ul class="flex gap-8">
          <For each={departments}>
            {(department) => (
              <li class="group relative py-3">
                <a class="text-sm font-semibold text-stone-800 hover:text-emerald-800" href={department.href}>
                  {department.name}
                </a>
                <ul class="invisible absolute left-0 top-full w-56 rounded-lg border border-stone-200 bg-white p-3 shadow-lg group-hover:visible">
                  <For each={department.sections}>
                    {(section) => (
                      <li>
                        <a class="block rounded px-2 py-1.5 text-sm text-stone-700 hover:bg-stone-100" href={section.href}>
                          {section.name}
                        </a>
                      </li>
                    )}
                  </For>
                </ul>
              </li>
            )}
          </For>
        </ul>
      </nav>
    </header>
  );
}

export function Footer() {
  return (
    <footer class="mt-16 border-t border-stone-200 bg-stone-100">
      <div class="mx-auto grid max-w-7xl gap-8 px-4 py-12 sm:grid-cols-2 sm:px-6 lg:grid-cols-5 lg:px-8">
        <div class="lg:col-span-1">
          <p class="text-lg font-bold text-emerald-900">Summit Outfitters</p>
          <p class="mt-2 text-sm text-stone-600">
            A member-owned cooperative since 1974. Members get a yearly dividend on what they buy, early access to sales, and free repairs
            at every store.
          </p>
        </div>
        <For each={help}>
          {(column) => (
            <div>
              <h2 class="text-sm font-semibold uppercase tracking-wide text-stone-900">{column.title}</h2>
              <ul class="mt-3 space-y-2">
                <For each={column.links}>
                  {(link) => (
                    <li>
                      <a class="text-sm text-stone-600 hover:text-emerald-800" href={link.href}>
                        {link.name}
                      </a>
                    </li>
                  )}
                </For>
              </ul>
            </div>
          )}
        </For>
      </div>
      <div class="border-t border-stone-200">
        <p class="mx-auto max-w-7xl px-4 py-6 text-xs text-stone-500 sm:px-6 lg:px-8">
          Free standard shipping on orders over 75 dollars to the contiguous United States. Returns are free for 60 days, worn or not.
          Prices and availability may change. Copyright 2026 Summit Outfitters Cooperative.
        </p>
      </div>
    </footer>
  );
}
