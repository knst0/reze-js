const CACHE = "reze-test";
const MANIFEST = "/reze-assets.json";

self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (event) => event.waitUntil(self.clients.claim()));

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  if (event.request.method !== "GET" || url.origin !== self.location.origin) return;
  event.respondWith(respond(event.request, url));
});

async function respond(request, url) {
  if (url.pathname === MANIFEST) return networkFirst(request);
  if (await isListed(url.pathname)) return cacheFirst(request);
  return networkFirst(request);
}

async function isListed(pathname) {
  const cache = await caches.open(CACHE);
  const manifest = await cache.match(MANIFEST);
  if (manifest === undefined) return false;
  const { files } = await manifest.json();
  return files.some((file) => new URL(file, self.location.href).pathname === pathname);
}

async function cacheFirst(request) {
  const cache = await caches.open(CACHE);
  const hit = await cache.match(request);
  if (hit !== undefined) return hit;
  const response = await fetch(request);
  if (response.ok) await cache.put(request, response.clone());
  return response;
}

async function networkFirst(request) {
  const cache = await caches.open(CACHE);
  try {
    const response = await fetch(request);
    if (response.ok) await cache.put(request, response.clone());
    return response;
  } catch {
    const cached = await cache.match(request);
    if (cached !== undefined) return cached;
    return new Response('<!doctype html><html><body><p id="sw-offline">offline</p></body></html>', {
      status: 503,
      headers: { "content-type": "text/html; charset=utf-8" },
    });
  }
}
