const assert = require('node:assert/strict');
const fs = require('node:fs/promises');

// Clear site storage while retaining the authenticator, as on a user's second install.
module.exports = async function verifyRecovery(page, ctx, base) {
  if (process.env.YAP_QA_ONBOARDING_ONLY) await verifyUnclaimedRestore(ctx.browser(), base);
  const cdp = await ctx.newCDPSession(page);
  await cdp.send('WebAuthn.enable');
  const addAuthenticator = supportsEncryption => cdp.send('WebAuthn.addVirtualAuthenticator',{options:{
    protocol:'ctap2',ctap2Version:'ctap2_1',transport:'internal',hasResidentKey:true,hasUserVerification:true,
    isUserVerified:true,automaticPresenceSimulation:true,hasPrf:supportsEncryption,hasHmacSecret:supportsEncryption}});
  const ordinaryPasskey=await addAuthenticator(false);
  const publicIdentity = () => page.evaluate(async () => {
    const {load}=await import('./store.js');
    const state=await load(); if(!state)return null;
    const {wallet}=state;
    const record=wallet.chain.at(-1).payload;
    return {account:record.account,devices:record.devices};
  });
  const first=await publicIdentity();
  await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'You',exact:true}).click(); await page.locator('#settings').click();
  await page.locator('#open-backup').click();
  await page.locator('#copy-key:enabled').waitFor();
  // Exercise Copy without putting the fixture signing material on the workstation clipboard.
  await page.evaluate(()=>{
    let copied='';
    Object.defineProperty(navigator,'clipboard',{value:{
      writeText:async value=>{copied=value;}, readText:async()=>copied
    }});
  });
  // Keep the temporary account out of the workstation's real password manager.
  await page.evaluate(()=>{
    navigator.credentials.store=async credential=>{
      window.fixtureEntry={id:credential.id,name:credential.name,password:credential.password,type:credential.type};
      return credential;
    };
  });
  await page.locator('#save-managed-key').click();
  await page.getByText('Check your password manager for the Yap recovery key.').waitFor();
  const managed=await page.evaluate(()=>window.fixtureEntry);
  assert.equal(managed.id,'Yap recovery · '+first.account);
  await page.locator('#copy-key').click();
  const recoveryKey=await page.evaluate(()=>navigator.clipboard.readText());
  assert.ok(/^yap1-[a-f0-9]{64}-[a-f0-9]{8}$/.test(recoveryKey),'a versioned recovery key was copied');
  await page.locator('#recovery summary').click();
  await page.locator('#protect-account').click();
  await page.getByText(/Your passkey was created, but it can’t protect/).waitFor();
  const registered=await cdp.send('WebAuthn.getCredentials',{authenticatorId:ordinaryPasskey.authenticatorId});
  assert.equal(registered.credentials.length,1);
  await cdp.send('WebAuthn.removeVirtualAuthenticator',{authenticatorId:ordinaryPasskey.authenticatorId});
  await addAuthenticator(true);
  await page.locator('#protect-account').click();
  const backup=page.locator('#download-recovery');
  await backup.waitFor({timeout:30000});
  const downloadPromise=page.waitForEvent('download');
  await backup.click();
  const download=await downloadPromise;
  const buffer=await fs.readFile(await download.path());
  assert.deepEqual(Object.keys(JSON.parse(buffer)).sort(),['cipher','credential','format','iv','origin','salt']);
  async function freshProfile() {
    await page.goto('about:blank');
    await cdp.send('Storage.clearDataForOrigin',{origin:new URL(base).origin,storageTypes:'all'});
    await page.goto(base);
    await page.getByRole('heading',{name:'Welcome to Yap'}).waitFor();
    await page.locator('#welcome-restore-existing').click();
    await page.locator('#welcome-restore').click();
  }
  await freshProfile();
  const guest = await publicIdentity();
  assert.equal(guest,null);
  await page.locator('#restore-key').fill(recoveryKey.slice(0,-1));
  await page.locator('#use-key').click();
  await page.getByText(/Check the recovery key/).waitFor();
  assert.notEqual((await publicIdentity())?.account,first.account);
  assert.ok(managed.password===recoveryKey,'the manager received the selected recovery key');
  await page.evaluate(entry=>{
    navigator.credentials.get=async options=>{
      if(options.password && options.mediation==='required') return entry;
      throw Error('Expected a user-selected password');
    };
  },managed);
  await page.locator('#get-managed-key').click();
  await page.getByText('Key selected. Choose Restore account to continue.').waitFor();
  await page.locator('#use-key').click();
  await page.locator('#recovery').waitFor({state:'hidden'});
  await page.locator('#posts[data-feed-state=primary]').waitFor({timeout:65000});
  await verifyRestoredInbox(page);
  const fromNote=await publicIdentity();
  assert.equal(fromNote.account,first.account);
  assert.equal(fromNote.devices.length,first.devices.length+1);
  await freshProfile();
  const fileGuest = await publicIdentity();
  async function choose(data) {
    const chooser=page.waitForEvent('filechooser');
    await page.locator('#restore-account').click();
    await (await chooser).setFiles({name:'yap-test.yap-recovery',mimeType:'application/json',buffer:data});
  }
  const damaged=JSON.parse(buffer);
  const bytes=Buffer.from(damaged.cipher,'base64'); bytes[0]^=1; damaged.cipher=bytes.toString('base64');
  await choose(Buffer.from(JSON.stringify(damaged)));
  await page.getByText(/This passkey could not unlock the file/).waitFor();
  assert.deepEqual(await publicIdentity(),fileGuest);
  await choose(buffer);
  try {
    await page.locator('#recovery').waitFor({state:'hidden',timeout:35000});
  } catch (error) {
    throw new Error('Restore did not complete: '+await page.locator('#recovery-status').textContent(),{cause:error});
  }
  await page.locator('#posts[data-feed-state=primary]').waitFor({timeout:65000});
  const recovered=await publicIdentity();
  assert.equal(recovered.account,first.account);
  assert.equal(recovered.devices.length,fromNote.devices.length+1);
  assert(first.devices.every(device=>recovered.devices.includes(device)));
  console.log('PASS ordinary passkey fallback, password-manager recovery key, tamper rejection and passkey-file recovery with fresh signing devices.');
};

async function verifyUnclaimedRestore(browser, base) {
  const previous = await browser.newContext();
  let key;
  try {
    const page = await previous.newPage();
    await page.goto(base);
    await page.locator('#welcome-create').waitFor();
    key = await page.evaluate(async () => {
      const home = await import('./network.js');
      const {browser_recovery_key} = await import('./runtime/yap.js');
      await home.start('create', 'Unclaimed neighbor');
      return browser_recovery_key(JSON.stringify(await home.confirmAccount()));
    });
  } finally { await previous.close(); }

  const restored = await browser.newContext();
  try {
    const page = await restored.newPage();
    await page.goto(base);
    await page.locator('#welcome-restore-existing').click();
    await page.locator('#welcome-restore').click();
    await page.locator('#restore-key').fill(key);
    await page.locator('#use-key').click();
    const field = page.locator('#welcome-username-local');
    await field.waitFor({timeout:65000});
    assert.equal(await page.locator('#neighborhood').isVisible(),false);
    await field.fill('restored_neighbor');
    await page.getByRole('button',{name:'Claim username'}).click();
    await page.locator('#posts[data-feed-state=primary]').waitFor({timeout:65000});
    assert.equal(await page.evaluate(async () => (await (await import('./store.js')).load()).username), 'restored_neighbor@neighborhood.test');
    console.log('PASS restored account without a registered name requires an acknowledged username.');
  } finally { await restored.close(); }
}

async function verifyRestoredInbox(page) {
  const confirmedUsername = await page.evaluate(async () => (await (await import('./store.js')).load()).username);
  assert.equal(confirmedUsername, 'library_reader@neighborhood.test');
  await page.getByRole('button',{name:'Inbox',exact:true}).click();
  await page.getByText(/This account’s inbox is linked to another installation of Yap/).waitFor({timeout:65000});
  const blockedMessage = await page.evaluate(async () => {
    const home = await import('./network.js');
    try { await home.sendMessage({peer:'synthetic',label:'Fixture neighbor'},'Unsent fixture'); return false; }
    catch (error) { return error.message === home.otherMessagingDevice; }
  });
  assert.ok(blockedMessage, 'The browser keeps unreadable mailbox keys and refuses to queue a send');
}

module.exports.verifyWebKitRestore = async function verifyWebKitRestore(page, browser, base) {
  await verifyUnclaimedRestore(browser, base);
  const key = await page.evaluate(async () => {
    const home = await import('./network.js');
    const {browser_recovery_key} = await import('./runtime/yap.js');
    return browser_recovery_key(JSON.stringify(await home.confirmAccount()));
  });
  const restored = await browser.newContext();
  try {
    const fresh = await restored.newPage();
    await fresh.goto(base);
    await fresh.locator('#welcome-restore-existing').click();
    await fresh.locator('#welcome-restore').click();
    await fresh.locator('#restore-key').fill(key);
    await fresh.locator('#use-key').click();
    await fresh.locator('#posts[data-feed-state=primary]').waitFor({timeout:65000});
    await verifyRestoredInbox(fresh);
    console.log('PASS WebKit recovery-key restore retains its registered username and explains the mailbox binding.');
  } finally { await restored.close(); }
};
