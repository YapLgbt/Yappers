const {chromium,webkit}=require('playwright');
const assert=require('node:assert/strict');
const fs=require('node:fs/promises');
const http=require('node:http');
const path=require('node:path');
const currentAssets=path.resolve(__dirname, '../dist');
const upgradeFrom=process.env.YAP_UPGRADE_FROM;
let assets=upgradeFrom ? path.resolve(upgradeFrom) : currentAssets;
const fixture=process.argv[2];
const expectedProductionHome='114f92b50e66dd84a14e0ff9b857b0c254565609b00b0cb9ef85f29e864efe5b';
const mime={'.html':'text/html','.js':'text/javascript','.wasm':'application/wasm','.css':'text/css',
  '.svg':'image/svg+xml','.png':'image/png','.jpg':'image/jpeg','.ttf':'font/ttf','.woff2':'font/woff2','.webmanifest':'application/manifest+json'};
const server=http.createServer(async (req,res)=>{
  try {
    const url=new URL(req.url,'http://localhost');
    if(url.pathname==='/.well-known/yap-home.json'){
      res.writeHead(200,{'Content-Type':'application/json'});return res.end(await fs.readFile(fixture));
    }
    const file=path.resolve(assets,'.'+(url.pathname.endsWith('/')?url.pathname+'index.html':url.pathname));
    if(!file.startsWith(assets+path.sep)) throw Error();
    const bytes=await fs.readFile(file);
    const headers={'Content-Type':mime[path.extname(file)]||'application/octet-stream'};
    if (url.pathname.startsWith('/app/')) headers['Content-Security-Policy']="default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; connect-src 'self' https: wss:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'";
    res.writeHead(200,headers);res.end(bytes);
  } catch {res.writeHead(404);res.end();}
});
const listen=port=>new Promise(resolve=>server.listen(port,'127.0.0.1',resolve));
const stop=()=>new Promise(resolve=>{server.close(resolve);server.closeAllConnections();});
(async()=>{
 const manifest=JSON.parse(await fs.readFile(fixture));
 assert.notEqual(manifest.join.serverAddr.id,expectedProductionHome,'Use an isolated test federation.');
 await listen(0);
 const port=server.address().port;
 const engine=process.env.YAP_BROWSER || 'chrome';
 const browser=engine==='webkit' ? await webkit.launch() : await chromium.launch({channel:'chrome',headless:true});
 try {
  const ctx=await browser.newContext({viewport:{width:390,height:844},isMobile:true,hasTouch:true,deviceScaleFactor:1});
  const page=await ctx.newPage();
  const errors=[];
  page.on('pageerror',e=>errors.push(String(e)));
  const base='http://localhost:'+port+'/app/';
  await page.goto(base);
  await page.getByRole('heading',{name:'Welcome to Yap'}).waitFor();
  assert.equal(await page.evaluate(async()=>await (await import('./store.js')).load()),null);
  if (!upgradeFrom) {
    const storedEntries = await page.evaluate(() => new Promise((resolve, reject) => {
      const open = indexedDB.open('yap-web-v6');
      open.onerror = () => reject(open.error);
      open.onsuccess = () => {
        const db = open.result;
        const count = db.transaction('device').objectStore('device').count();
        count.onsuccess = () => { db.close(); resolve(count.result); };
        count.onerror = () => { db.close(); reject(count.error); };
      };
    }));
    assert.equal(storedEntries, 0, 'Welcome should not create a device key before the first write.');
    const policies = page.getByRole('navigation',{name:'Policies'});
    const terms = await policies.getByRole('link',{name:'Terms of Service'}).boundingBox();
    const privacy = await policies.getByRole('link',{name:'Privacy Policy'}).boundingBox();
    assert.equal(terms.y,privacy.y);
    assert.equal(await page.getByRole('button',{name:/^(Play|Pause) motion$/}).count(),0);
    if (process.env.YAP_QA_SCREENSHOTS) await page.screenshot({path:path.join(process.env.YAP_QA_SCREENSHOTS,'welcome-intro-'+engine+'.png')});
    if (process.env.YAP_QA_ONBOARDING_ONLY) {
      await page.emulateMedia({colorScheme:'dark',reducedMotion:'reduce'});
      const print = page.locator('.welcome-art .cloth');
      await print.locator('img').evaluate(image => image.decode());
      assert.deepEqual(await print.boundingBox(), await page.locator('#welcome').boundingBox(),
        'The Estuary should fill the welcome screen edge to edge');
      if (await print.locator('canvas').isVisible()) {
        const first = await print.screenshot();
        assert.ok(first.equals(await print.screenshot()), 'The Welcome print stays still with system Reduce Motion enabled');
      }
      if (process.env.YAP_QA_SCREENSHOTS) await page.screenshot({path:path.join(process.env.YAP_QA_SCREENSHOTS,'welcome-dark-reduced-motion-'+engine+'.png')});
      await page.emulateMedia({colorScheme:'light',reducedMotion:'no-preference'});
    }
    await page.locator('#welcome-restore-existing').click();
  }
  await page.locator('#welcome-restore').click();
  if (!upgradeFrom) await page.locator('#restore-key:focus').waitFor();
  await page.locator('#recovery [data-close]').click();
  await page.locator('#recovery').waitFor({state:'hidden'});
  if (!upgradeFrom) await page.getByRole('button',{name:'Back',exact:true}).click();
  await page.locator('#welcome-create').click();
  const suggestion=await page.locator('#welcome-alias').inputValue();
  if (!upgradeFrom) {
    assert.equal(suggestion, '');
    for (const blank of ['', '   ']) {
      await page.locator('#welcome-alias').fill(blank);
      await page.locator('#welcome-join').click();
      await page.locator('#welcome-alias[aria-invalid=true]:focus').waitFor();
      assert.equal(await page.evaluate(async () => (await import('./store.js')).load()), null);
    }
  }
  await page.locator('#suggest-name').click();
  assert.notEqual(await page.locator('#welcome-alias').inputValue(),suggestion);
  if (!upgradeFrom) {
    await page.locator('#welcome-alias').fill('x');
    await page.locator('#welcome-join').click();
    await page.locator('#welcome-alias[aria-invalid="true"]:focus').waitFor();
  }
  await page.locator('#welcome-alias').fill('Library neighbor');
  if (process.env.YAP_QA_SCREENSHOTS) await page.screenshot({path:path.join(process.env.YAP_QA_SCREENSHOTS,'welcome-name-'+engine+'.png')});
  if (!upgradeFrom) {
    await page.evaluate(() => new Promise(locked => {
      navigator.locks.request('yap-web-delivery-v1', () => new Promise(release => {
        window.resumeUsernameCheck = release;
        locked();
      }));
    }));
  }
  await page.locator('#welcome-join').click();
  if (!upgradeFrom) {
    const draft = page.locator('#welcome-username-local:focus');
    await draft.fill('early_draft');
    assert(await page.locator('#welcome-username-form button[type=submit]').isDisabled());
    await page.evaluate(() => window.resumeUsernameCheck());
  }
  if (process.env.YAP_QA_UNSUPPORTED_ONLY) {
    const unsupported = page.getByRole('alert').filter({hasText:'This home does not support usernames'});
    await unsupported.waitFor({timeout:65000});
    assert.equal(await page.locator('#welcome-enter').isVisible(),false);
    assert.equal(await page.locator('#neighborhood').isVisible(),false);
    await page.getByRole('button',{name:'Try again'}).click();
    await unsupported.waitFor({timeout:65000});
    await page.reload();
    await unsupported.waitFor({timeout:65000});
    assert.deepEqual(errors,[]);
    console.log('PASS an unconfigured home cannot complete username onboarding, including after retry and reload.');
    return;
  }
  if (!upgradeFrom) {
    const username = page.locator('#welcome-username-local');
    await page.locator('#welcome-username-form button[type=submit]:enabled').waitFor({timeout:65000});
    assert.equal(await username.inputValue(), 'early_draft');
    assert(await username.evaluate(field => document.activeElement === field));
    assert.equal(await page.locator('#welcome-enter').isVisible(), false);
    if (process.env.YAP_QA_SCREENSHOTS) await page.screenshot({path:path.join(process.env.YAP_QA_SCREENSHOTS,'welcome-username-'+engine+'.png')});
    await page.evaluate(async () => {
      const home = await import('./network.js');
      await home.modify(s => ({...s,setupStep:'backup',username:'unconfirmed@neighborhood.test'}));
      let blocked = false;
      try { await home.verifyWelcomeUsername(true); } catch { blocked = true; }
      if (!blocked) throw Error('An unconfirmed cached name admitted a new account');
      await home.queue({kind:'post',anonymous:true,topic:'Plans',body:'Unsent fixture'}).then(
        () => { throw Error('An anonymous post escaped onboarding'); },
        error => { if (!String(error).includes('Finish setting up')) throw error; }
      );
    });
    await page.reload();
    await username.waitFor({timeout:65000});
    await username.fill('9invalid');
    const claim = page.locator('#welcome-username-form button[type=submit]');
    assert.equal(await claim.isDisabled(), true);
    await username.fill(process.env.YAP_QA_PARITY_ONLY ? 'bookclub' : 'library_reader');
    await claim.click();
  }
  await page.getByRole('heading',{name:'Keep your account'}).waitFor();
  await page.reload();
  await page.locator('#welcome-enter').click();
  await page.locator('#posts[data-feed-state=primary]').waitFor({timeout:65000});
  if (process.env.YAP_QA_ONBOARDING_ONLY) {
    assert.equal(await page.evaluate(async () => (await (await import('./store.js')).load()).username), 'library_reader@neighborhood.test');
    const recovery = require('./recovery.cjs');
    if (engine === 'webkit') await recovery.verifyWebKitRestore(page,ctx.browser(),base);
    else await recovery(page,ctx,base);
    assert.deepEqual(errors,[]);
    console.log('PASS mandatory browser username, pending setup reload, confirmed restored identity, and device-specific inbox.');
    return;
  }
  if(process.env.YAP_QA_PARITY_ONLY){
    await require('./parity.cjs')(page,ctx,base,engine);
    assert.deepEqual(errors,[]);
    return;
  }
  async function verifyPull(surface) {
    await page.waitForFunction(()=>!document.getElementById('refresh').disabled);
    const lastSync=await page.evaluate(async()=> (await (await import('./store.js')).load()).lastSync);
    await page.evaluate(surface=>{
      const feed=document.getElementById(surface);
      if (surface==='neighborhood') window.scrollTo(0,0); else feed.scrollTop=0;
      for(const [type,y] of [['touchstart',40],['touchmove',150],['touchend',0]]) {
        const event=new Event(type,{bubbles:true,cancelable:true});
        Object.defineProperty(event,'touches',{value:type==='touchend'?[]:[{clientX:80,clientY:y}]});
        feed.dispatchEvent(event);
        if(type==='touchmove' && document.getElementById('pull-feedback').textContent!=='Release to refresh') throw Error('Pull feedback missing');
      }
    },surface);
    await page.waitForFunction(async stamp=>(await (await import('./store.js')).load()).lastSync>stamp,lastSync,{timeout:65000});
  }
  await verifyPull('neighborhood');
  // Rehearse the same consent and receipt flow on a disposable, staffed test home.
  if (!upgradeFrom && process.env.YAP_QA_REPORTS === '1') {
    await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'You',exact:true}).click(); await page.locator('#settings').click();
    await page.getByRole('button',{name:'Report to your home',exact:true}).click();
    let dialog=page.getByRole('dialog',{name:'Report to your home'});
    await dialog.getByLabel('Evidence for the moderator').fill('yap1-fake-fixture');
    await dialog.getByRole('button',{name:'Review report'}).click();
    if (!upgradeFrom) await dialog.locator('#report-title:focus').waitFor();
    await dialog.getByRole('button',{name:'Send these details'}).click();
    await dialog.getByRole('alert').filter({hasText:'remove recovery keys'}).waitFor();
    await dialog.getByRole('button',{name:'Edit',exact:true}).click();
    await dialog.getByLabel('Evidence for the moderator').fill('Browser rehearsal: selected excerpt');
    await dialog.getByRole('button',{name:'Review report'}).click();
    const receipt=await dialog.getByLabel('Private receipt').inputValue();
    assert.ok(receipt.startsWith('yr1.'+manifest.join.serverAddr.id+'.'));
    await dialog.getByRole('button',{name:'Send these details'}).click();
    await page.getByText('Received. Awaiting review.',{exact:true}).waitFor();
    if (!upgradeFrom) await page.getByRole('dialog',{name:'Report status'}).locator('#report-title:focus').waitFor();
    await page.getByRole('dialog',{name:'Report status'}).getByRole('button',{name:'Done'}).click();
    await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'You',exact:true}).click(); await page.locator('#settings').click();
    await page.getByRole('button',{name:'Check a private receipt'}).click();
    dialog=page.getByRole('dialog',{name:'Report status'});
    await dialog.getByLabel('Private receipt').fill(receipt);
    await dialog.getByRole('button',{name:'Check status'}).click();
    await dialog.getByText('Received. Awaiting review.',{exact:true}).waitFor();
    await dialog.getByRole('button',{name:'Done'}).click();
    const persisted=await page.evaluate(async()=>JSON.stringify(await (await import('./store.js')).load()));
    assert.ok(!persisted.includes(receipt) && !persisted.includes('Browser rehearsal: selected excerpt'));
    console.log('PASS home-only report review, recovery-key rejection, submission and private receipt lookup.');
  }
  function viewFeed(name) {
    if (assets !== currentAssets) return page.locator(name === 'Nearby' ? '#feed-tab' : '#saved-tab').click();
    return page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name,exact:true}).click();
  }
  await viewFeed('Saved');
  await verifyPull('neighborhood');
  if (!upgradeFrom) await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'You',exact:true}).click();
  await page.locator('#settings').click();
  await verifyPull(upgradeFrom ? 'options' : 'preferences-body');
  await page.locator('#options [data-close]').click();
  const displayName = upgradeFrom ? 'Library neighbor' : 'West End reader';
  const handle = 'library_reader@neighborhood.test';
  if (!upgradeFrom) {
    await page.getByRole('button', {name:'Edit profile',exact:true}).click();
    const profileEditor = page.getByRole('dialog', {name:'Edit profile'});
    await profileEditor.getByRole('textbox', {name:'Display name',exact:true}).fill(displayName);
    await profileEditor.getByRole('button', {name:'Save profile',exact:true}).click();
    await profileEditor.waitFor({state:'hidden'});
  }
  await viewFeed('Nearby');
  const anonymousMode = page.getByRole('switch', {name: 'Anonymous mode'});
  async function assertNamedDraft() {
    const editor = page.locator('#editor');
    assert.equal(await anonymousMode.isChecked(), false);
    assert.equal(await editor.getByRole('textbox',{name:'Display name',exact:true,includeHidden:true}).count(),0);
    assert.equal(await editor.locator('#public-byline strong').innerText(), displayName);
    assert.equal(await editor.locator('#public-byline .signature-handle').innerText(), handle);
  }
  const title='A library walk '+Date.now();
  await page.locator('#compose').click();
  if (!upgradeFrom) {
    await assertNamedDraft();
    await page.locator('#send').click();
    await page.locator('#headline[aria-invalid="true"]:focus').waitFor();
    assert.equal(await page.locator('#headline').getAttribute('aria-describedby'), 'headline-error headline-limit');
    assert.equal(await page.locator('#body').getAttribute('aria-invalid'), 'true');
    assert.equal(await page.locator('#body').getAttribute('aria-describedby'), 'body-error body-limit');
    await anonymousMode.check();
    await page.locator('#headline').fill('Keep my draft');
    await page.keyboard.press('Escape');
    const discard = page.getByRole('dialog',{name:'Discard your draft?'});
    await discard.getByRole('button',{name:'Keep editing'}).click();
    assert.equal(await page.locator('#headline').inputValue(), 'Keep my draft');
    assert(await anonymousMode.isChecked(), 'Keep editing preserves this draft’s anonymous choice');
    await page.keyboard.press('Escape');
    await discard.getByRole('button',{name:'Discard draft'}).click();
    await page.locator('#editor').waitFor({state:'hidden'});
    await page.locator('#compose:focus').waitFor();
    await page.getByRole('button',{name:'Post',exact:true}).click();
    assert.equal(await page.locator('#headline').inputValue(), '');
    await assertNamedDraft();
  }
  await page.locator('#headline').fill(title);
  await page.locator('#body').fill('Meet at the west entrance.');
  if (!upgradeFrom) {
    assert.equal(await page.locator('#headline').getAttribute('aria-invalid'), null);
    assert.equal(await page.locator('#body').getAttribute('aria-invalid'), null);
  }
  if (upgradeFrom) {
    await page.locator('#posting-identity').selectOption('named');
    assert.equal(await page.locator('#send').isDisabled(),true);
    await page.locator('#editor-username button').click();
    await page.locator('#username-local').fill('library_reader');
    await page.getByRole('button',{name:'Claim username',exact:true}).click();
    await page.locator('#username-dialog').waitFor({state:'hidden'});
  }
  await page.locator('#send').click();
  const card=page.locator('article').filter({has:page.getByRole('heading',{name:title,exact:true})});
  await card.waitFor({timeout:65000});
  await card.getByText('library_reader@neighborhood.test',{exact:true}).waitFor();
  assert((await card.locator('.byline').first().textContent()).includes(displayName));
  if (!upgradeFrom) assert.equal(await card.locator('.avatar').count(), 0);
  await card.getByRole('button',{name:'Save',exact:true}).click();
  await viewFeed('Saved');
  if (!upgradeFrom) {
    const viewport = page.viewportSize();
    await page.setViewportSize({ width: 1440, height: 900 });
    const listPost = page.locator('#posts').locator('article').filter({ hasText: title });
    const replies = listPost.getByRole('button', { name: 'Replies (0)', exact: true });
    await replies.click();
    const pane = page.getByRole('region', { name: 'Conversation' });
    await pane.getByRole('heading', { name: 'Comments' }).waitFor();
    assert(await listPost.isVisible(), 'The saved post stays visible beside its thread at wide widths');
    assert(await pane.getByRole('heading', { name: 'Comments' }).evaluate(e => document.activeElement === e));
    await pane.getByRole('button', { name: 'Back to saved posts' }).click();
    await pane.waitFor({ state: 'hidden' });
    assert(await replies.evaluate(e => document.activeElement === e));
    await page.setViewportSize(viewport);
  }
  await card.getByRole('button',{name:'Replies (0)'}).click();
  if (!upgradeFrom) {
  await page.getByRole('heading',{name:'Conversation',exact:true}).waitFor();
  await page.goBack();
  await card.getByRole('button',{name:'Replies (0)'}).waitFor();
  await page.goForward();
  await page.getByRole('heading',{name:'Comments',exact:true}).waitFor();
  }
  assert.equal(await page.locator('#editor').isVisible(),false);
  assert.equal(await page.locator('textarea:focus').count(),0);
  await card.getByRole('button',{name:'Write a reply'}).click();
  if (upgradeFrom) assert.equal(await page.locator('#posting-identity').inputValue(),'named');
  else await assertNamedDraft();
  await page.locator('#body').fill('See you there.');
  await page.locator('#send').click();
  await card.getByText('See you there.').waitFor({timeout:65000});
  await card.getByText('library_reader@neighborhood.test',{exact:true}).nth(1).waitFor({timeout:65000});
  if (!upgradeFrom) await card.locator('.reply:focus').getByText('See you there.').waitFor();
  assert.equal(await card.getByText(handle,{exact:true}).count(),2);
  const namedReply = card.locator('.reply').filter({hasText:'See you there.'});
  if (!upgradeFrom) {
    await namedReply.getByText(displayName,{exact:true}).waitFor();
    await card.getByRole('button',{name:'Write a reply'}).click();
    await assertNamedDraft();
    await anonymousMode.check();
    assert.equal(await page.locator('#public-byline').getByText(handle,{exact:true}).count(),0);
    await page.locator('#body').fill('Bringing a spare library card.');
    await page.locator('#send').click();
    const privateReply = card.locator('.reply').filter({hasText:'Bringing a spare library card.'});
    await privateReply.getByText('Anonymous neighbor',{exact:true}).waitFor();
    await privateReply.getByRole('button',{name:'Delete reply'}).waitFor({timeout:65000});
    assert.equal(await privateReply.getByText(handle,{exact:true}).count(),0);
  }
  await page.evaluate(()=>document.fonts.ready);
  for (const selector of ['h2',':scope > p','.reply p']) {
    assert.match(await card.locator(selector).first().evaluate(el=>getComputedStyle(el).fontFamily),/Geist Serif/);
  }
  assert.match(await card.locator('.byline').first().evaluate(el=>getComputedStyle(el).fontFamily),/Geist Sans/);
  if (process.env.YAP_QA_SCREENSHOTS) {
    await card.screenshot({path:path.join(process.env.YAP_QA_SCREENSHOTS,'post-type-'+engine+'.png')});
  }
  await viewFeed('Nearby');
  await page.evaluate(()=>navigator.serviceWorker.ready);
  await page.reload();await page.locator('#posts[data-feed-state=primary]').waitFor({timeout:65000});
  // WebKit's emulated offline mode rejects document navigation before the service
  // worker sees it. Remove the HTTP origin to exercise a real cold, cached reload.
  assert.equal(await page.evaluate(()=>!!navigator.serviceWorker.controller),true);
  await stop();
  if(engine!=='webkit') await ctx.setOffline(true);
  await page.goto(base+'?offline', {waitUntil:'domcontentloaded'});
  await page.locator('#compose:enabled').waitFor({timeout:20000});
  await card.waitFor();
  for (const family of ['Geist Sans','Geist Serif']) {
    assert.ok(await page.evaluate(async f=>(await document.fonts.load('18px "'+f+'"')).length>0,family),'offline '+family);
  }
  if(engine==='webkit') await ctx.setOffline(true);
  await page.locator('#compose').click();
  if (upgradeFrom) {
    assert.equal(await page.locator('#posting-identity').inputValue(),'named');
    await page.locator('#posting-identity').selectOption('anonymous');
  } else {
    await assertNamedDraft();
    await anonymousMode.check();
  }
  await page.locator('#headline').fill(title+' offline');
  await page.locator('#body').fill('Written while offline.');await page.locator('#send').click();
  if (upgradeFrom) await page.getByText('Post waiting to send · '+title+' offline').waitFor();
  else await page.getByRole('article').filter({has:page.getByRole('heading',{name:title+' offline',exact:true})}).getByText('Waiting to send',{exact:true}).waitFor();
  const queued = await page.evaluate(async () => (await (await import('./store.js')).load()).outbox.map(e => e.payload.action));
  const queuedPost = queued.find(action => action.headline === title+' offline');
  assert.equal(queuedPost?.anonymous,true);
  assert.equal(queuedPost.displayName,'Anonymous neighbor');
  assert.ok(!queuedPost.profile);
  let accountBefore;
  if (upgradeFrom) {
    const local=await page.evaluate(async()=>{
      const state=await (await import('./store.js')).load();
      return {account:state.wallet.chain.at(-1).payload.account,queued:state.outbox.length};
    });
    assert.equal(local.queued,1);
    accountBefore=local.account;
    await page.goto('about:blank');
    assets=currentAssets;
  }
  await listen(port);
  await ctx.setOffline(false);
  if (upgradeFrom) {
    await page.goto(new URL('/terms/',base).href);
    await page.evaluate(async()=>{
      const registration=await navigator.serviceWorker.getRegistration('/app/');
      if (!registration) throw Error('Original installation missing');
      await registration.update();
      const worker=registration.installing || registration.waiting || registration.active;
      await new Promise((resolve,reject)=>{
        const timeout=setTimeout(()=>reject(Error('New worker did not activate')),30000);
        function changed() {
          if (worker.state==='activated') {clearTimeout(timeout);resolve();}
          if (worker.state==='redundant') {clearTimeout(timeout);reject(Error('Offline cache install failed'));}
        }
        worker.addEventListener('statechange',changed);
        changed();
      });
    });
    const upgraded=await page.goto(base);
    assert.match(await upgraded.text(), /src="\/app\/start\.[^"]+\.js"/, 'Updated worker must serve the migrated shell');
    await page.locator('#posts[data-feed-state=primary]').waitFor({timeout:65000});
    assert.equal(await page.evaluate(async()=>(await (await import('./store.js')).load()).wallet.chain.at(-1).payload.account),accountBefore);
    await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'Saved',exact:true}).click(); await card.waitFor(); await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'Nearby',exact:true}).click();
  }
  await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'You',exact:true}).click(); await page.locator('#settings').click(); await page.locator('#refresh-preferences').click(); await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'Nearby',exact:true}).click();
  await page.getByRole('heading',{name:title+' offline',exact:true}).waitFor({timeout:65000});
  await page.setViewportSize({width:1100,height:780});await page.emulateMedia({colorScheme:'dark'});
  await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'You',exact:true}).click(); await page.locator('#settings').click();
  await page.getByText('Hide anonymous posts and replies', {exact:true}).click();
  assert(await page.getByRole('switch', {name:'Hide anonymous posts and replies'}).isChecked());
  await page.getByRole('button',{name:'Done',exact:true}).click(); await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'Nearby',exact:true}).click();
  // Own posts remain visible just as in the native app.
  assert.equal(await card.count(),1);
  const offlinePosts=await page.getByRole('heading',{name:title+' offline',exact:true}).count();
  assert.equal(offlinePosts,1);
  const {join}=await (await ctx.request.get(new URL('/.well-known/yap-home.json',base).href)).json();
  const received=await page.evaluate(async ({join, title})=>{
   const {default:init,BrowserClient,create_wallet}=await import('./runtime/yap.js');await init();
   const w=JSON.parse(create_wallet(join.serverAddr.id));
   const c=await BrowserClient.open(JSON.stringify({join,authorSecret:w.deviceSecret,nextSequence:1,accountChain:w.chain}));
   try{
    const feed=JSON.parse(await c.request('{"type":"feed"}'));
    const offline=feed.posts.find(p=>p.headline===title+' offline');
    const published=feed.posts.find(p=>p.headline===title);
    if (!offline || offline.username || offline.profile || offline.author!=='Anonymous neighbor') throw Error('Anonymous post exposed a name or profile');
    let conflict=false;
    try { await c.request(JSON.stringify({type:'social',op:{kind:'claimUsername',name:'LIBRARY_READER'}})); }
    catch(e){conflict=String(e).includes('taken');}
    if(!conflict) throw Error('A second account could claim the same username');
    return {titles:feed.posts.map(p=>p.headline), published};
   }
   finally{await c.close();}
  },{join,title});
  assert.equal(received.titles.filter(x=>x===title+' offline').length,1);
  assert.equal(received.published.author,displayName);
  assert.equal(received.published.username,handle);
  const publishedReply=received.published.replies.find(reply=>reply.body==='See you there.');
  assert.equal(publishedReply.author,displayName);
  assert.equal(publishedReply.username,handle);
  if (!upgradeFrom) {
    const privateReply=received.published.replies.find(reply=>reply.body==='Bringing a spare library card.');
    assert.equal(privateReply.author,'Anonymous neighbor');
    assert.ok(!privateReply.username && !privateReply.visibility?.peer && !privateReply.profile);
  }
  await page.locator('#feed-filter').click();
  const filters = page.getByRole('dialog', { name: 'Feed filters' });
  const newest = filters.getByRole('radio', { name: 'New', exact: true });
  const popular = filters.getByRole('radio', { name: 'Popular', exact: true });
  await newest.focus();
  await page.keyboard.press('ArrowRight');
  assert(await popular.isChecked());
  await page.keyboard.press('Escape');
  await page.locator('#feed-filter:focus').waitFor();
  await page.locator('#feed-filter').click();
  assert(await newest.isChecked(), 'Dismissing the filter dialog discards the pending selection');
  await popular.click();
  await filters.getByRole('radio', { name: 'Plans', exact: true }).click();
  await page.locator('#filter-form button[type=submit]').click();
  await page.getByRole('heading',{name:'No posts in Plans'}).waitFor();
  await page.getByRole('button',{name:'Show all topics'}).click();
  const repliesButton=card.getByRole('button',{name:upgradeFrom ? 'Replies (1)' : 'Replies (2)'});
  if (await repliesButton.getAttribute('aria-expanded')!=='true') await repliesButton.click();
  await card.getByRole('button',{name:'Write a reply'}).click();
  await assertNamedDraft();
  await page.locator('#editor [data-close]').click();
  page.once('dialog',dialog=>dialog.accept());
  await namedReply.getByRole('button',{name:'Delete reply'}).click();
  await card.getByText('See you there.',{exact:true}).waitFor({state:'detached',timeout:65000});
  await card.locator('button[aria-label="Post options"]').click();
  page.once('dialog',dialog=>dialog.accept());
  await page.getByRole('menuitem',{name:'Delete post'}).click();
  await card.waitFor({state:'hidden'});
  await page.reload(); await page.locator('#posts[data-feed-state=primary]').waitFor({timeout:65000});
  assert.equal(await card.count(),0);
  const offlineCard=page.locator('article').filter({has:page.getByRole('heading',{name:title+' offline',exact:true})});
  await offlineCard.locator('button[aria-label="Post options"]').click();
  await page.getByRole('menuitem',{name:'Hide on this device'}).click();
  await offlineCard.waitFor({state:'hidden'});
  await require('./inbox.cjs')(page,ctx,fixture,engine);
  if(engine==='chrome') await require('./recovery.cjs')(page,ctx,base);
  assert.deepEqual(errors,[]);
  console.log('PASS browser Iroh enrollment, post, bookmark, reply, offline reload, durable queued write, reconnect and independent receipt.');
 } finally {await browser.close();if(server.listening)await stop();}
})().catch(e=>{console.error(e);process.exitCode=1});
