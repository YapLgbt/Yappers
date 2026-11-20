const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
module.exports = async function (page, ctx, base, engine) {
  const button = (name, root = page) => root.getByRole('button', { name, exact: true });
  const dialog = page.getByRole('dialog');
  const state = async (fn) =>
    fn(await page.evaluate(async () => await (await import('/app/store.js')).load()));
  const settle = () => page.waitForFunction(() => !document.getElementById('refresh').disabled);
  const audit = (label) => require('./accessibility.cjs')(page, label);
  const shots = process.env.YAP_QA_SCREENSHOTS;
  await require('./demo.cjs')(ctx.browser(),base);
  await require('./feed-motion.cjs')(ctx.browser(),base);
  if (engine === 'chrome') await require('./background.cjs')(page, ctx, base);
  async function layout(name) {
    for (const width of [320, 390, 1100]) {
      await page.setViewportSize({ width, height: 840 });
      assert(
        await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
        'Viewport overflow on ' + name + ' at ' + width,
      );
      if (shots)
        await page.screenshot({
          path: path.join(shots, name + '-' + engine + '-' + width + '.png'),
        });
    }
    await page.setViewportSize({ width: 390, height: 844 });
  }
  if (await page.evaluate(() => matchMedia('(pointer: coarse)').matches))
    assert(await page.locator('#refresh').isHidden(), 'Touch devices use pull to refresh');
  await audit('Nearby');
  await page.goto(base + 'demo/');
  await page.getByRole('heading', { name: 'Local demo' }).waitFor();
  await button('Save', page.locator('article').first()).click();
  await page.getByRole('tab', { name: 'Saved' }).click();
  assert.equal(await page.locator('article:visible').count(), 1);
  await audit('Local demo');
  await page.goto(base);
  await page.locator('#posts[data-feed-state=primary]').waitFor({ timeout: 65000 });
  const nav = page.getByRole('navigation', {name: 'Neighborhood'});
  // Sample the first painted frame so short transitions are observable even in CI.
  async function changePage(label) {
    return button(label, nav).evaluate(async trigger => {
      trigger.click();
      await new Promise(requestAnimationFrame);
      const pane = document.querySelector('.reading-list');
      const animation = pane.getAnimations().find(a => a.effect?.target === pane);
      if (!animation) return 0;
      const start = animation.effect.getKeyframes()[0].transform;
      animation.finish();
      return new DOMMatrix(start).m41;
    });
  }
  await page.emulateMedia({reducedMotion: 'no-preference'});
  assert(await changePage('Saved') > 0, 'Saved enters from the right of Nearby');
  assert(await changePage('Nearby') < 0, 'Nearby returns from the left');
  assert.equal(await changePage('Nearby'), 0, 'Reselecting the active tab keeps its content still');
  await page.emulateMedia({reducedMotion: 'reduce'});
  assert.equal(await changePage('You'), 0, 'Reduce Motion keeps navigation instant');
  await page.emulateMedia({reducedMotion: null});
  await audit('You');
  await button('Preferences').click();
  const hideAnonymous = dialog.getByRole('switch', { name: 'Hide anonymous posts and replies', exact: true });
  await dialog.getByText('Hide anonymous posts and replies', {exact:true}).waitFor();
  await dialog.getByText('Only approved contacts in my inbox', {exact:true}).waitFor();
  await hideAnonymous.focus();
  await page.keyboard.press('Space');
  await page.waitForFunction(async () => (await (await import('/app/store.js')).load()).hideAnonymous);
  await button('Done', dialog).click();
  await button('Preferences').click();
  assert(await hideAnonymous.isChecked(), 'Anonymous-post preference should persist when reopened');
  await dialog.getByText('Hide anonymous posts and replies', {exact:true}).click();
  await page.waitForFunction(async () => !(await (await import('/app/store.js')).load()).hideAnonymous);
  await page.emulateMedia({ colorScheme: 'dark' });
  await audit('Dark preferences');
  if (shots) await page.screenshot({path:path.join(shots, 'preferences-dark-' + engine + '.png')});
  await page.emulateMedia({ colorScheme: 'light' });
  await button('Done', dialog).click();
  await button('Edit profile').click();
  const editor = page.getByRole('dialog', { name: 'Edit profile' });
  await editor.waitFor();
  await editor.locator('.avatar img[src="/app/default-avatar.png"]').waitFor();
  const profilePage = await page.evaluate(() => ({ url: location.href, scroll: scrollY }));
  await button('Cancel', editor).click();
  await editor.waitFor({ state: 'hidden' });
  await page.waitForFunction(() => document.activeElement?.getAttribute('aria-label') === 'Edit profile');
  assert.deepEqual(await page.evaluate(() => ({ url: location.href, scroll: scrollY })), profilePage);
  await button('Edit profile').click();
  const friends = editor.getByRole('button', { name: 'Friends', exact: true });
  await friends.focus();
  await page.keyboard.press('Space');
  assert.equal(await friends.getAttribute('aria-pressed'), 'true');
  await editor.getByLabel('Pronouns').fill('they/them');
  await editor.getByLabel('Bio', { exact: true }).fill('Book club regular');
  await editor
    .getByLabel('Profile photo', { exact: true })
    .setInputFiles(path.join(__dirname, '../static/app/icon-192.png'));
  const cropPreview = editor.locator('canvas.crop-preview');
  await cropPreview.waitFor();
  const zoomIn = button('Zoom in', editor);
  const zoomOut = button('Zoom out', editor);
  const moveLeft = button('Move photo left', editor);
  const moveUp = button('Move photo up', editor);
  const resetCrop = button('Reset crop', editor);
  const zoomValue = async () => parseInt(await editor.locator('.crop-zoom-value').innerText(), 10);
  const cropImage = () => cropPreview.evaluate((canvas) => canvas.toDataURL());
  const waitForCropChange = (previous) =>
    page.waitForFunction((image) => document.querySelector('canvas.crop-preview').toDataURL() !== image, previous);
  assert.equal(await editor.getByRole('slider').count(), 0);
  assert.equal(await zoomValue(), 100);
  assert(await zoomOut.isDisabled() && await moveLeft.isDisabled() && await moveUp.isDisabled());
  const initialCrop = await cropImage();
  for (let step = 0; step < 4; step++) await zoomIn.press('Space');
  assert.equal(await zoomValue(), 200);
  assert(await zoomOut.isEnabled() && await moveLeft.isEnabled() && await moveUp.isEnabled());
  await waitForCropChange(initialCrop);
  const beforeNudge = await cropImage();
  await moveLeft.press('Enter');
  await waitForCropChange(beforeNudge);
  await resetCrop.press('Enter');
  assert.equal(await zoomValue(), 100);
  assert.equal(await cropImage(), initialCrop);
  for (let step = 0; step < 4; step++) await zoomIn.press('Enter');
  page.once('dialog',prompt=>prompt.dismiss());
  await page.keyboard.press('Escape');
  assert.equal(await editor.getByLabel('Bio',{exact:true}).inputValue(),'Book club regular');
  await cropPreview.scrollIntoViewIfNeeded();
  const crop = await cropPreview.boundingBox();
  const beforeDrag = await cropImage();
  await page.mouse.move(crop.x+crop.width/2,crop.y+crop.height/2);
  await page.mouse.down();await page.mouse.move(crop.x+crop.width*.7,crop.y+crop.height*.7);await page.mouse.up();
  await waitForCropChange(beforeDrag);
  if(engine==='chrome'){
    const touch=await ctx.newCDPSession(page), x=crop.x+crop.width/2, y=crop.y+crop.height/2;
    try {
      const fingers=spread=>[{x:x-spread,y,id:1},{x:x+spread,y,id:2}];
      await touch.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:fingers(25)});
      await touch.send('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:fingers(40)});
      await touch.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
      assert(await zoomValue() > 250, 'Pinching the preview should zoom into the photo.');
    } finally {await touch.detach();}
  }
  await button('Cancel', editor).focus();
  await page.keyboard.press('Shift+Tab');
  await friends.and(page.locator(':focus')).waitFor();
  await audit('Profile editing');
  await layout('profile-crop');
  // A failed save keeps the editor and its draft. Escape and Tab stay inside the dialog while editing.
  await page.evaluate(() => {
    const prototype = SubtleCrypto.prototype;
    const encrypt = prototype.encrypt;
    prototype.encrypt = async function (algorithm, key, data) {
      // Only fail the draft write. A concurrent background refresh can also write to device storage.
      if (new TextDecoder().decode(data).includes('"bio":"Book club regular"')) {
        prototype.encrypt = encrypt;
        throw new Error('Storage unavailable');
      }
      return encrypt.call(this, algorithm, key, data);
    };
  });
  await button('Save profile', editor).click();
  await editor.getByRole('alert').waitFor();
  assert.equal(await editor.getByLabel('Bio', { exact: true }).inputValue(), 'Book club regular');
  await button('Save profile', editor).click();
  await editor.waitFor({ state: 'hidden' });
  await page.waitForFunction(
    () => document.activeElement?.getAttribute('aria-label') === 'Edit profile',
  );
  assert(
    await state((s) => s.publicProfile.photo.length > 32 && s.publicProfile.photo.length <= 384 * 1024),
  );
  const portrait = page.locator('.profile-summary.own .avatar canvas:visible');
  await portrait.waitFor();
  assert(await portrait.evaluate(canvas => canvas.width === 1024 && canvas.height === 1024), 'The profile renders the 1024-pixel crop');
  assert.equal(await page.locator('.profile-summary.own .avatar img').count(), 0);
  assert.deepEqual(await state((s) => s.publicProfile.hereFor), ['Friends']);
  await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'Nearby',exact:true}).click();
  // Returning to You remounts the portrait. Hold the decoder until the first frame is checked.
  await page.evaluate(() => {
    const decode = window.createImageBitmap.bind(window);
    let resume;
    const gate = new Promise(resolve => { resume = resolve; });
    window.createImageBitmap = async (...args) => { await gate; return decode(...args); };
    window.resumePhotoDecode = () => { window.createImageBitmap = decode; resume(); };
  });
  await page.goBack();
  await button('Edit profile').waitFor();
  assert.equal(await page.locator('.profile-summary.own .avatar img').count(), 0,
    'A custom portrait must not flash the default during decoding');
  await page.evaluate(() => window.resumePhotoDecode());
  await page.locator('.profile-summary.own .avatar canvas:visible').waitFor();
  await page.goForward();
  await button('Choose a town').waitFor();
  await ctx.grantPermissions(['geolocation'], { origin: new URL(base).origin });
  await ctx.setGeolocation({ latitude: 47.60657, longitude: -122.33299 });
  await button('Choose a town').click();
  await button('Use my location', dialog).click();
  await dialog.locator('p[aria-live]').filter({hasText:/WA, US/}).waitFor();
  const gpsLabel=await dialog.locator('p[aria-live]').innerText();
  await button('Apply area', dialog).click();
  await dialog.waitFor({ state: 'hidden' });
  await settle();
  assert.deepEqual(await state((s) => s.nearby.center), {
    label:gpsLabel,
    lat100: 4761,
    lon100: -12233,
  });
  await page.locator('.place-heading').click();
  await dialog.getByRole('searchbox', { name: 'Search for a city or town' }).pressSequentially('Seattle');
  await dialog
    .getByRole('button', { name: /Seattle, WA, US/ })
    .first()
    .click();
  await dialog.getByLabel('Feed radius (miles)').fill('15');
  await audit('Town selection');
  await button('Apply area', dialog).click();
  await dialog.waitFor({ state: 'hidden' });
  await settle();
  assert.equal(await state((s) => s.nearby.radiusMiles), 15);
  await page.getByRole('heading', {name:'No posts to show within 15 miles'}).waitFor();
  await button('Write a post',page.locator('#posts')).click();
  await dialog.getByText('Seattle, WA, US', {exact:true}).waitFor();
  await page.keyboard.press('Escape');
  await dialog.waitFor({state:'hidden'});
  await button('Widen to 30 miles').click();
  await page.getByText('Within 30 miles', {exact:true}).waitFor();
  await settle();
  assert.equal(await state(s=>s.nearby.center.label),'Seattle, WA, US');
  await page.locator('.place-heading').click();
  await button('All cities', dialog).click();
  await button('Apply area', dialog).click();
  await dialog.waitFor({ state: 'hidden' });
  await settle();
  await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'Groups',exact:true}).click();
  await page.setViewportSize({width:320,height:840});
  const title = page.getByRole('heading', { name: 'Groups', exact: true });
  assert(await title.evaluate(el => el.offsetHeight < parseFloat(getComputedStyle(el).lineHeight) * 1.5),
    'The directory title must fit on one line on a small phone');
  await page.setViewportSize({width:390,height:844});
  await button('New group').click();
  await audit('Create group');
  const name = 'QA book group';
  await dialog.getByLabel('Group name').fill(name);
  await dialog.getByLabel('Description').fill('A group for our neighborhood book club.');
  await dialog.getByRole('checkbox', { name: 'City or town group' }).check();
  await dialog.getByRole('textbox', { name: 'City or town', exact: true }).fill('Seattle');
  await dialog.getByLabel('Region',{exact:true}).fill('WA');
  await dialog.getByLabel('Country code').fill('US');
  assert.equal(await dialog.locator('form').evaluate(f=>f.checkValidity()),true,'Group fields are valid before creating');
  await button('Create group', dialog).click();
  const groupHeading=page.getByRole('heading',{name,exact:true});
  await Promise.race([groupHeading.waitFor({timeout:65000}),dialog.getByRole('alert').waitFor({timeout:65000}).then(async()=>{throw new Error(await dialog.getByRole('alert').innerText())})]);
  await settle();
  await page.locator('#compose').click();
  const composerIdentity = page.getByRole('switch', {name:'Anonymous mode'});
  assert.equal(await composerIdentity.isChecked(),false);
  await composerIdentity.check();
  await page.locator('#headline').fill('Group-only invitation');
  await page.locator('#body').fill('Bring your favorite book.');
  await page.locator('#send').click();
  const card = page.locator('article').filter({ hasText: 'Group-only invitation' });
  await card.waitFor({ timeout: 65000 });
  const groupPostId = await card.getAttribute('data-post-id');
  const tabs = page.getByRole('navigation', {name:'Neighborhood'});
  await button('Nearby',tabs).click();
  await button('Choose a town').waitFor();
  await page.goBack();
  await groupHeading.waitFor();
  await card.waitFor();
  assert.equal(await button('Groups',tabs).getAttribute('aria-current'),'page');
  await page.goForward();
  await button('Choose a town').waitFor();
  assert.equal(await card.count(),0);
  await page.goBack();
  await groupHeading.waitFor();
  await card.waitFor();
  await button('Upvote', card).waitFor({ timeout: 65000 });
  assert(await state(s => !s.posts.find(post => post.id === groupPostId).profile), 'Anonymous post exposed a profile');
  assert.equal(await card.locator('.avatar').count(), 0);
  assert.equal(await card.getByRole('button', { name: 'Anonymous neighbor', exact: true }).count(), 0);
  assert.equal(await card.getByRole('button', { name: 'Message', exact: true }).count(), 0);
  await button('Upvote', card).click();
  await card.locator('[aria-label=Upvote][aria-pressed=true]:enabled').waitFor();
  await button('Upvote', card).click();
  await card.locator('[aria-label=Upvote][aria-pressed=false]:enabled').waitFor();
  await layout('group-feed');
  await page.emulateMedia({ colorScheme: 'dark' });
  await audit('Dark group feed');
  await page.emulateMedia({ colorScheme: 'light' });
  const menu = button('Post options', card);
  await menu.focus();
  await page.keyboard.press('Enter');
  await page.getByRole('menu').waitFor();
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Escape');
  await page.waitForFunction(
    () => document.activeElement?.getAttribute('aria-label') === 'Post options',
  );
  await button('Save', card).click();
  await page.locator('#compose').click();
  await page.locator('#headline').fill('Signed profile in the group');
  await page.locator('#body').fill('See you at book club.');
  assert.equal(await composerIdentity.isChecked(),false);
  assert.equal(await page.locator('#public-byline strong').innerText(),'Library neighbor');
  assert.equal(await state(s => s.username), 'bookclub@neighborhood.test');
  assert(await page.locator('#send').isEnabled(), 'The claimed username is available when posting by name');
  await page.locator('#send').click();
  const named=page.locator('article').filter({hasText:'Signed profile in the group'});
  await named.waitFor({timeout:65000});
  await button('Library neighbor',named).click();
  await dialog.getByText('Book club regular',{exact:true}).waitFor();
  await dialog.locator('canvas[width="1024"][height="1024"]:not([hidden])').waitFor();
  await page.keyboard.press('Escape');await dialog.waitFor({state:'hidden'});

  await button('Save', named).click();
  // Refresh the saved group article while viewing the home feed.
  await page.evaluate(async () => {
    const home = await import('/app/network.js');
    const original = home.profile.publicProfile;
    const group = home.profile.activeGroup;
    const id = home.profile.posts.find(p => p.headline === 'Signed profile in the group').id;
    await home.selectGroup(null);
    await home.setPublicProfile('Updated neighbor', {...original, photo:'', bio:'Revised bio'}, home.profile.profileRevision);
    await home.sync();
    const post = home.profile.savedPosts[id];
    if (post.author !== 'Updated neighbor' || post.profile.photo || post.profile.bio !== 'Revised bio')
      throw new Error('Historical group post retained stale profile details');
    await home.selectGroup(group);
    await home.setPublicProfile('Library neighbor', original, home.profile.profileRevision);
    await home.sync();
  });

  await button('Saved', named).click();
  await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'Groups',exact:true}).click();
  await page.getByRole('tab',{name:'Local',exact:true}).click();
  await button('Choose a town').click();
  await button('Seattle, WA, US',dialog).click();
  await dialog.waitFor({state:'hidden'});
  await button('Seattle, WA, US').waitFor();
  assert.equal(await state(s=>s.groupLocality.city),'Seattle');
  const groupSearch = page.getByRole('searchbox',{name:'Search groups'});
  await groupSearch.fill('No matching name');
  await page.getByRole('heading', { name: 'No matching groups' }).waitFor();
  await groupSearch.fill('book');
  await button('Open',page.getByRole('region',{name,exact:true})).click();
  await settle();
  await ctx.setOffline(true);
  await page.locator('#compose').click();await page.locator('#headline').fill('Queued group-only post');
  await page.locator('#body').fill('Deliver this to the group after reconnecting.');await page.locator('#send').click();
  const groupQueue = page.getByRole('article').filter({has:page.getByRole('heading',{name:'Queued group-only post',exact:true})});
  await groupQueue.getByText('Waiting to send',{exact:true}).waitFor();
  await button('Back to nearby').click();
  await page.locator('.place-heading').waitFor();
  assert.equal(await groupQueue.count(),0);
  await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'Groups',exact:true}).click();
  await button('Open',page.getByRole('region',{name,exact:true})).click();
  await card.waitFor(); // Previously opened group restored without a network request.
  await button('Back to nearby').click();
  await ctx.setOffline(false);
  await page.locator('.place-heading').waitFor();
  const network = await page.evaluateHandle(() => import('/app/network.js'));
  await page.waitForFunction(home => home.profile.outbox.length === 0, network, {timeout:65000});
  await network.dispose();
  assert.equal(await page.locator('article').filter({hasText:'Queued group-only post'}).count(),0);
  await settle();
  assert.equal(await card.count(), 0);
  await ctx.setOffline(true);
  await page.evaluate(async()=>{
    const net=await import('/app/network.js');
    await net.queue({kind:'postV2',topic:'Chatter',headline:'Discard this upload',body:'Queued while offline',displayName:'Anonymous neighbor',anonymous:true});
  });
  await page.locator('#inbox-tab').click();
  await button('Inbox settings').click(); await page.getByRole('menuitem',{name:'Blocked accounts'}).click();
  await dialog.getByText('No blocked accounts.',{exact:true}).waitFor();
  page.once('dialog',d=>d.dismiss());
  await button('Discard pending uploads',dialog).click();
  assert.equal(await state(s=>s.outbox.length),1);
  // Another tab can enqueue new work after confirmation. Discard only reviewed IDs.
  const reviewed=await state(s=>s.outbox.map(e=>e.id));
  await page.evaluate(async ids=>{
    const net=await import('/app/network.js');
    await net.queue({kind:'postV2',topic:'Chatter',headline:'Keep this upload',body:'From another tab',displayName:'Anonymous neighbor',anonymous:true});
    await net.discardUploads(ids);
  },reviewed);
  assert.equal(await state(s=>s.outbox[0].payload.action.headline),'Keep this upload');
  page.once('dialog',d=>d.accept());await button('Discard pending uploads',dialog).click();
  await page.waitForFunction(async()=>!(await(await import('/app/store.js')).load()).outbox.length);
  await button('Done',dialog).click();await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'Nearby',exact:true}).click();
  await ctx.setOffline(false);
  await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'Saved',exact:true}).click();
  await card.waitFor();
  await menu.click();
  await page.getByRole('menuitem', { name: 'Mute conversation' }).click();
  await card.waitFor({ state: 'hidden' });
  await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'You',exact:true}).click();
  await button('Preferences').click();
  await button('Unmute', dialog).click();
  await button('Done', dialog).click();
  await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'Saved',exact:true}).click();
  await card.waitFor();
  await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'Groups',exact:true}).click();
  await page.getByRole('tab', { name: 'Joined' }).click();
  await page.getByRole('heading', { name, exact: true }).waitFor();
  await page.getByRole('tab', { name: 'Local' }).focus();
  await page.keyboard.press('ArrowRight');
  await page.getByRole('tab', { name: 'Worldwide', selected: true }).waitFor();
  await button('Private groups').click();
  await button('Create private group').click();
  await page.getByLabel('Group name', { exact: true }).fill('Private QA attachments');
  await button('Create private group').click();
  await button('New post').click();
  const privateGroups = page.locator('.private-groups');
  await privateGroups.getByLabel('Headline', { exact: true }).fill('File for group');
  const attachment = Buffer.alloc(300_000, 'Private attachment fixture.');
  await privateGroups.locator('input[type=file]').setInputFiles({
    name: 'group-fixture.txt', mimeType: 'text/plain', buffer: attachment,
  });
  await button('Post', privateGroups).click();
  const attachmentButton = button('Download group-fixture.txt');
  await attachmentButton.waitFor({timeout:65000});
  const attachmentDownload = page.waitForEvent('download', {timeout:65000});
  await attachmentButton.click();
  const downloaded = await attachmentDownload;
  assert.equal(downloaded.suggestedFilename(), 'group-fixture.txt');
  assert.deepEqual(await fs.readFile(await downloaded.path()), attachment);
  await page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'You',exact:true}).click();
  await button('Preferences').click();
  await page.getByRole('combobox', { name: 'Minimum account age for posts' }).selectOption({ label: '7 days' });
  await page.waitForFunction(
    async () => {
      const s = await (await import('/app/store.js')).load();
      return !s.pendingPrivacy && s.inboxPolicy?.feedDays === 7;
    },
    {},
    { timeout: 65000 },
  );
  await button('Done', dialog).click();
  await button('Account data').click();
  await button('Save a full backup', dialog).click();
  const phrase = 'test backup for disposable home';
  await dialog.getByLabel('Backup passphrase', { exact: true }).fill(phrase);
  await dialog.getByLabel('Confirm passphrase').fill('a different backup passphrase');
  await button('Download backup', dialog).click();
  await dialog.getByLabel('Confirm passphrase').and(page.locator('[aria-invalid="true"]:focus')).waitFor();
  await dialog.getByLabel('Confirm passphrase').fill(phrase);
  const download = page.waitForEvent('download', { timeout: 120000 });
  await button('Download backup', dialog).click();
  const item = await download;
  assert.equal(item.suggestedFilename(), 'Yap-Recovery.age');
  let data = await fs.readFile(await item.path());
  assert.equal(data.subarray(0, 21).toString(), 'age-encryption.org/v1');
  if (engine === 'chrome')
    data = require('node:child_process').execFileSync(
      'python3',
      [path.join(__dirname, 'native-archive.py')],
      { input: data, timeout: 180000, maxBuffer: 32 * 1024 * 1024 },
    );
  const account = await state((s) => s.wallet.chain.at(-1).payload.account);
  const destination = process.env.YAP_QA_DESTINATION
    ? JSON.parse(await fs.readFile(process.env.YAP_QA_DESTINATION)).join
    : null;
  const home = await state((s) => s.join.serverAddr.id);
  if (destination) assert.notEqual(destination.serverAddr.id, home);
  const other = await ctx.browser().newContext();
  try {
    const fresh = await other.newPage();
    await fresh.goto(base);
    await fresh.locator('#welcome-restore-existing').click();
    await button('Restore a full backup', fresh).click();
    const restore = fresh.getByRole('dialog');
    await restore
      .getByLabel('Backup file (.age)')
      .setInputFiles({ name: 'test.age', mimeType: 'application/octet-stream', buffer: data });
    await restore.getByLabel('Backup passphrase').fill(phrase);
    if (destination) {
      await restore.getByText('Move to a different home', { exact: true }).click();
      await restore.getByLabel('New home invitation').fill(JSON.stringify(destination));
    }
    await restore.getByLabel('Make this my active device').check();
    await button('Restore account', restore).click();
    if (destination) {
      const username = fresh.getByRole('textbox', { name: 'Username on destination.test' });
      await username.waitFor({ timeout: 120000 });
      assert.equal(await fresh.locator('#posts[data-feed-state=primary]').isVisible(), false);
      await fresh.reload();
      await username.waitFor({ timeout: 65000 });
      assert.equal(await username.inputValue(), '');
      assert.equal(await fresh.evaluate(async () => (await (await import('/app/store.js')).load()).username), null);
      await username.fill('moved_reader');
      await fresh.getByRole('button', { name: 'Claim username', exact: true }).click();
    }
    await fresh.locator('#posts[data-feed-state=primary]').waitFor({ timeout: 120000 });
    assert.deepEqual(
      await fresh.evaluate(async () => {
        const s = await (await import('/app/store.js')).load();
        return [
          s.wallet.chain.at(-1).payload.account,
          s.publicProfile.bio,
          s.ownedEvents.length > 0,
          s.join.serverAddr.id,
          s.wallet.chain.at(-1).payload.home,
          s.username,
          s.setupStep,
        ];
      }),
      [account, 'Book club regular', true, destination?.serverAddr.id || home, destination?.serverAddr.id || home,
        destination ? 'moved_reader@destination.test' : 'bookclub@neighborhood.test', null],
    );
    await fresh.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'You',exact:true}).click();
    await button('Account data', fresh).click();
    await button('Reset this browser', fresh).click();
    await fresh.getByLabel('Clear my local data').check();
    await button('Reset browser', fresh).click();
    await fresh.getByRole('heading', { name: 'Welcome to Yap' }).waitFor();
  } finally {
    await other.close();
  }
  console.log(
    'PASS profile crop, keyboard focus, towns, radius, group feed isolation, encrypted attachment download, voting, account-age sync, portable backup import, device reset and responsive layouts (' +
      engine +
      ').',
  );
};
