// Module service workers can use the same Rust client and delivery lock as an open tab.
import { start, sync, profile } from './network.js';
import { load } from './store.js';

async function deliver() {
  const saved = await load();
  if (
    !saved ||
    saved.welcomePending ||
    saved.setupStep ||
    saved.pendingRecovery ||
    saved.pendingTransfer
  )
    return;
  if (await start()) {
    await sync();
    if (profile.outbox.length || profile.directState?.outbox.length || profile.pendingPrivacy ||
        profile.pendingApprovals?.length || profile.pendingUnblocks.length || profile.blocks.some(b=>!b.handle))
      throw new Error('Delivery is waiting for the home.');
  }
}
self.addEventListener('sync', (event) => {
  if (event.tag === 'yap-delivery') event.waitUntil(deliver());
});
self.addEventListener('periodicsync', (event) => {
  if (event.tag === 'yap-refresh') event.waitUntil(deliver());
});
