const cacheName = 'yap-web-__YAP_BUILD__';
const files = /* offline files */ [];
const urls = files.map(path => new URL(path, self.registration.scope).href);
const shell = new URL('index.html', self.registration.scope);

self.addEventListener('install', event => {
  event.waitUntil(caches.open(cacheName).then(cache =>
    cache.addAll(urls.map(url => new Request(url, {cache:'reload'})))
  ));
});
self.addEventListener('activate', event => {
  event.waitUntil((async () => {
    for (const key of await caches.keys()) {
      if (key.startsWith('yap-web-') && key !== cacheName) await caches.delete(key);
    }
    await self.clients.claim();
  })());
});
self.addEventListener('fetch', event => {
  const {request} = event;
  if (request.method !== 'GET') return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  const navigation = request.mode === 'navigate' &&
    (url.pathname === shell.pathname || url.pathname === new URL('./', shell).pathname);
  const target=navigation?shell.href:request.mode==='navigate' && url.pathname.endsWith('/')?new URL('index.html',url).href:url.href;
  if (!urls.includes(target)) return;
  event.respondWith((async () => {
    const cache = await caches.open(cacheName);
    return (await cache.match(target)) || fetch(request);
  })());
});
