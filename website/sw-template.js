const build = '__YAP_BUILD__';
const cacheName = 'yap-web-' + build;
const assets = /* offline assets */ [];
const urls = new Set(assets.map(({path}) => new URL(path, self.registration.scope).href));
const shell = new URL('index.html', self.registration.scope);

self.addEventListener('install', event => {
  event.waitUntil((async () => {
    const cache = await caches.open(cacheName);
    await cache.addAll(assets.map(({path, integrity}) => new Request(path, {cache:'reload', integrity})));
    await self.skipWaiting();
  })());
});
self.addEventListener('message', event => {
  if (event.data?.type === 'yap-get-build') event.source?.postMessage({type:'yap-build', build});
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
  if (!urls.has(target)) return;
  event.respondWith((async () => {
    const cache = await caches.open(cacheName);
    return (await cache.match(target)) || fetch(request);
  })());
});

self.addEventListener('push', event => {
  let data;
  try {data=event.data?.json();} catch {return;}
  if (!/^[a-f0-9]{64}$/.test(data?.route) || !/^\d+$/.test(data?.revision)) return;
  event.waitUntil((async()=>{
    const existing=await self.registration.getNotifications({tag:data.route});
    if(existing.some(n=>Number(n.data?.revision)>=Number(data.revision))) return;
    await self.registration.showNotification('Yap',{
      body:'You have an Inbox update.', icon:'/app/icon-192.png',
      tag:data.route, data:{route:data.route, revision:data.revision}
    });
  })());
});
self.addEventListener('notificationclick', event=>{
  const route=event.notification.data?.route;
  event.notification.close();
  if(!/^[a-f0-9]{64}$/.test(route)) return;
  event.waitUntil((async()=>{
    const windows=await self.clients.matchAll({type:'window',includeUncontrolled:true});
    const page=windows.find(w=>new URL(w.url).pathname==='/app/');
    if(page){await page.focus();page.postMessage({type:'yap-open-inbox',route});}
    else await self.clients.openWindow('/app/#inbox='+route);
  })());
});
