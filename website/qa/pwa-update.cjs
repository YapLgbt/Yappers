// Run with YAP_BROWSER=chrome or webkit and three static dist directories:
// node website/qa/pwa-update.cjs <pre-fix> <fixed-A> <fixed-B>
// Build A and B with npm run build. A change to any precached asset makes a distinct B.
// Each scenario uses a fresh browser profile. No Home or enrollment is required.
const assert = require('node:assert/strict');
const { createHash } = require('node:crypto');
const fs = require('node:fs/promises');
const http = require('node:http');
const path = require('node:path');
const { chromium, webkit } = require('playwright');

const wasm = '/app/runtime/yap_bg.wasm';
const timeout = 30_000;
const mime = { '.js': 'text/javascript', '.html': 'text/html', '.wasm': 'application/wasm',
  '.css': 'text/css', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.webmanifest': 'application/manifest+json' };

async function release(directory) {
  const root = path.resolve(directory);
  const shell = await fs.readFile(path.join(root, 'app/index.html'), 'utf8');
  const build = shell.match(/<meta name="yap-build" content="([^"]+)"/)?.[1] ?? null;
  return { root, build };
}

function fixtureServer() {
  const fixture = { release: null, corrupt: false, corruptions: 0, offline: false };
  const server = http.createServer(async (req, res) => {
    if (fixture.offline) return req.socket.destroy();
    try {
      const { pathname } = new URL(req.url, 'http://localhost');
      if (!/^\/(app|_app|assets)\//.test(pathname)) { res.writeHead(404).end(); return; }
      const file = path.resolve(fixture.release.root, '.' + (pathname.endsWith('/') ? pathname + 'index.html' : pathname));
      assert(file.startsWith(fixture.release.root + path.sep));
      const body = await fs.readFile(file);
      if (fixture.corrupt && pathname === wasm) {
        body[body.length - 1] ^= 1;
        fixture.corruptions++;
      }
      res.writeHead(200, {
        'Content-Type': mime[path.extname(file)] ?? 'application/octet-stream',
        'Cache-Control': 'no-cache',
        'Content-Security-Policy': "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; connect-src 'self'"
      });
      res.end(body);
    } catch { res.writeHead(404).end(); }
  });
  return { fixture, server };
}

async function install(page, url) {
  // Observe real messages and full document loads. The probe never asks for a
  // build or initiates an update, either of which could conceal a missing app listener.
  await page.addInitScript(() => {
    sessionStorage.qaLoads = String(Number(sessionStorage.qaLoads || 0) + 1);
    window.qaBuild = null;
    navigator.serviceWorker.addEventListener('message', e => {
      if (e.source === navigator.serviceWorker.controller && e.data?.type === 'yap-build') window.qaBuild = e.data.build;
    });
  });
  await page.goto(url);
  await page.getByRole('heading', { name: 'Welcome to Yap' }).waitFor();
  await page.waitForFunction(() => !!navigator.serviceWorker.controller);
}

async function expectBuild(page, build) {
  await page.waitForFunction(expected =>
    document.querySelector('meta[name="yap-build"]')?.content === expected && window.qaBuild === expected,
    build, { timeout });
}

async function online(context) {
  await context.setOffline(true);
  await context.setOffline(false);
}

async function legacyUpgrade(context, url, fixture, old, a) {
  fixture.release = old;
  const tab = await context.newPage();
  await install(tab, url);
  const other = await context.newPage();
  await install(other, url);
  fixture.release = a;
  // v48 has no application update listener. Trigger the browser's worker check
  // while BOTH legacy clients are still controlled, then navigate just one tab.
  await tab.evaluate(async () => {
    const previous = navigator.serviceWorker.controller;
    await (await navigator.serviceWorker.ready).update();
    window.qaOldController = previous;
  });
  await tab.waitForFunction(() => navigator.serviceWorker.controller !== window.qaOldController);
  await tab.goto(url);
  await expectBuild(tab, a.build);
  assert.equal(await other.evaluate(() => document.querySelector('meta[name="yap-build"]')?.content ?? null), null);
  console.log('PASS legacy tab loaded A on next navigation with another legacy tab open');
}

async function automaticUpgrade(context, url, fixture, a, b) {
  fixture.release = a;
  const tab = await context.newPage();
  await install(tab, url);
  await expectBuild(tab, a.build);
  assert.equal(await tab.evaluate(() => sessionStorage.qaLoads), '1', 'first install reloaded the shell');

  await tab.locator('#welcome-create').click();
  const alias = tab.locator('#welcome-alias');
  await alias.fill('Unsaved neighborhood alias');
  fixture.release = b;
  await online(context);
  await tab.waitForFunction(build => window.qaBuild === build, b.build, { timeout });
  assert.equal(await tab.locator('meta[name="yap-build"]').getAttribute('content'), a.build);
  assert.equal(await alias.inputValue(), 'Unsaved neighborhood alias');
  assert.equal(await tab.evaluate(() => sessionStorage.qaLoads), '1', 'an update interrupted Welcome');
  await tab.locator('#welcome-back').click();
  await expectBuild(tab, b.build);
  await tab.getByRole('heading', { name: 'Welcome to Yap' }).waitFor();
  assert.equal(await tab.evaluate(() => sessionStorage.qaLoads), '2', 'expected one automatic reload');
  console.log('PASS first installation, online update, preserved Welcome input, automatic reload to B');
}

async function failedUpgrade(context, url, fixture, a, b) {
  fixture.release = a;
  const tab = await context.newPage();
  await install(tab, url);
  await expectBuild(tab, a.build);
  await tab.evaluate(async () => {
    const reg = await navigator.serviceWorker.ready;
    reg.addEventListener('updatefound', () => {
      const worker = reg.installing;
      worker.addEventListener('statechange', () => { window.qaInstallState = worker.state; });
    });
  });
  fixture.release = b;
  fixture.corrupt = true;
  await online(context);
  await tab.waitForFunction(() => window.qaInstallState === 'redundant', null, { timeout });
  assert(fixture.corruptions > 0, 'new worker did not request the corrupt WASM');

  // Refuse socket connections even for the worker. Both WebKit and Chrome
  // can now reach the shell and runtime only through the previous offline cache.
  fixture.offline = true;
  const shell = await tab.goto(url);
  assert(shell?.fromServiceWorker(), 'offline shell did not come from the active worker');
  await expectBuild(tab, a.build);
  await tab.getByRole('heading', { name: 'Welcome to Yap' }).waitFor();
  const actual = await tab.evaluate(async pathname => {
    const response = await fetch(pathname);
    if (!response.ok) throw Error('offline runtime unavailable');
    const bytes = await crypto.subtle.digest('SHA-256', await response.arrayBuffer());
    return Array.from(new Uint8Array(bytes), n => n.toString(16).padStart(2, '0')).join('');
  }, wasm);
  const expected = createHash('sha256').update(await fs.readFile(path.join(a.root, wasm))).digest('hex');
  assert.equal(actual, expected, 'offline runtime differs from A');
  console.log('PASS corrupt B rejected, A shell and exact A WASM available offline');
}

async function main() {
  assert.equal(process.argv.length, 5, 'expected three dist directories: pre-fix, A, B');
  const [old, a, b] = await Promise.all(process.argv.slice(2).map(release));
  assert.equal(old.build, null, 'legacy fixture already implements the build handshake');
  assert(a.build && b.build && a.build !== b.build, 'A and B must contain distinct build IDs');
  const engine = process.env.YAP_BROWSER || 'chrome';
  assert(['chrome', 'webkit'].includes(engine), 'YAP_BROWSER must be chrome or webkit');
  const browser = engine === 'chrome' ? await chromium.launch({ channel: 'chrome' }) : await webkit.launch();
  const { server, fixture } = fixtureServer();
  try {
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    const url = 'http://127.0.0.1:' + server.address().port + '/app/';
    for (const run of [legacyUpgrade, automaticUpgrade, failedUpgrade]) {
      const context = await browser.newContext();
      context.setDefaultTimeout(timeout);
      try {
        if (run === legacyUpgrade) await run(context, url, fixture, old, a);
        else await run(context, url, fixture, a, b);
      } finally { await context.close(); }
    }
  } finally {
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
    await browser.close();
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
