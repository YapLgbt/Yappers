const assert = require('node:assert/strict');
module.exports = async function (page, ctx, base) {
  const cdp = await ctx.newCDPSession(page);
  let registration;
  cdp.on('ServiceWorker.workerRegistrationUpdated', ({ registrations }) => {
    registration = registrations.find((r) => r.scopeURL === base && !r.isDeleted) || registration;
  });
  await cdp.send('ServiceWorker.enable');
  await page.evaluate(async () => {
    const registration = await navigator.serviceWorker.ready;
    if (!registration.active.scriptURL.endsWith('/background-sw.js'))
      throw Error('Module worker unavailable');
    const net = await import('/app/network.js'),
      wasm = await import('/app/runtime/yap.js');
    // Reserve one real signed event without triggering a foreground send or registering a sync.
    await net.modify((s) => {
      const result = JSON.parse(
        wasm.prepare_action(
          JSON.stringify(s.wallet),
          s.join.community,
          JSON.stringify({
            kind: 'postV2',
            headline: 'Background delivery',
            body: 'Sent from the installed worker',
            anonymous: true,
            displayName: 'Anonymous neighbor',
            topic: 'Chatter',
          }),
        ),
      );
      return {
        ...s,
        wallet: { ...s.wallet, nextSequence: result.nextSequence },
        outbox: [...s.outbox, result.event],
      };
    });
  });
  // Keep a read-only test observer on the origin. No app window or foreground sync is running.
  await page.goto(new URL('/privacy-policy/', base).href);
  assert(registration);
  await cdp.send('ServiceWorker.dispatchSyncEvent', {
    origin: new URL(base).origin,
    registrationId: registration.registrationId,
    tag: 'yap-delivery',
    lastChance: false,
  });
  await page.waitForFunction(
    async () => {
      const s = await (await import('/app/store.js')).load();
      return !s.outbox.length && s.posts.some((p) => p.headline === 'Background delivery');
    },
    null,
    { timeout: 70000 },
  );
  await cdp.detach();
  await page.goto(base);
  await page.locator('#posts[data-feed-state=primary]').waitFor({ timeout: 65000 });
  console.log('PASS signed outbox delivery in the service worker with the PWA window closed.');
};
