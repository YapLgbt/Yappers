// One origin-scoped, non-extractable key protects the wallet, drafts in the outbox, and cache.
const database = new Promise((resolve, reject) => {
  const open = indexedDB.open('yap-web-v1', 1);
  open.onupgradeneeded = () => open.result.createObjectStore('device');
  open.onsuccess = () => resolve(open.result);
  open.onerror = () => reject(open.error);
});
async function records(mode, run) {
  const db = await database;
  return new Promise((resolve, reject) => {
    const tx = db.transaction('device', mode);
    const request = run(tx.objectStore('device'));
    tx.oncomplete = () => resolve(request.result);
    tx.onabort = () => reject(tx.error || new Error('Device storage unavailable'));
    tx.onerror = () => reject(tx.error);
  });
}
const get = id => records('readonly', store => store.get(id));
const put = (id, value) => records('readwrite', store => store.put(value, id));
const lockName = 'yap-web-device-v1';

async function deviceKey() {
  const saved = await get('key');
  if (saved) return saved;
  if (await get('state')) throw new Error('The browser key is missing. This profile cannot be opened.');
  const key = await crypto.subtle.generateKey({name:'AES-GCM', length:256}, false, ['encrypt','decrypt']);
  await put('key', key);
  return key;
}
async function read(key) {
  const record = await get('state');
  if (!record) return null;
  key ??= await deviceKey();
  const bytes = await crypto.subtle.decrypt({name:'AES-GCM', iv:record.iv}, key, record.cipher);
  return JSON.parse(new TextDecoder().decode(bytes));
}
export function update(change) {
  return navigator.locks.request(lockName, async () => {
    const key = await deviceKey();
    const state = change(await read(key));
    const iv = crypto.getRandomValues(new Uint8Array(12));
    const cipher = await crypto.subtle.encrypt({name:'AES-GCM', iv}, key, new TextEncoder().encode(JSON.stringify(state)));
    await put('state', {iv, cipher});
    return state;
  });
}
export function load() {
  return navigator.locks.request(lockName, () => read());
}

export function clearDevice(check) {
  return navigator.locks.request(lockName, async () => {
    check(await read());
    await records('readwrite', store => store.clear());
  });
}
