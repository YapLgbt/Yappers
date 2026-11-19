const assert = require('node:assert/strict');

module.exports = async function verifyFeedMotion(browser, base) {
  const context = await browser.newContext({viewport: {width: 390, height: 844}, reducedMotion: 'no-preference', serviceWorkers: 'block'});
  const source = `
    const post = id => ({id, author:'Neighbor', acceptedAt: Math.floor(Date.now()/1000)-600, topic:'Chatter', headline:'Post '+id, body:'In the neighborhood.', replies:[]});
    export let profile = {alias:'Robin', username:'robin@town.test', join:{community:'fixture'},
      posts:Array.from({length:18}, (_, index) => post(String(index))), lastSync:1,
      blocks:[], saved:[], pendingUnblocks:[], outbox:[]};
    export const updates = new EventTarget(), start=()=>true, sync=()=> 'primary', usernameDomain=()=> 'town.test', queuedPosts=()=>[];
    export const modify = async change => { profile = change(profile); updates.dispatchEvent(new Event('change')); };
    window.updateFeed = (change, id) => {
      let posts = profile.posts.filter(p => p.id !== id);
      if (change === 'replace') posts = [post('replacement-1'), post('replacement-2')];
      else if (change !== 'remove') posts.unshift(profile.posts.find(p => p.id === id) || post(id));
      profile = {...profile, posts};
      updates.dispatchEvent(new Event('change'));
    };
  `;
  try {
    const page = await context.newPage();
    await page.route('**/app/network.js*', route => route.fulfill({contentType:'application/javascript', body:source}));
    await page.goto(base);
    await page.locator('#posts[data-feed-state=primary] article').first().waitFor();
    assert.equal(await page.locator('#posts article time').first().textContent(), '10m ago');
    assert.equal(await page.locator('#feed-heading:focus').count(), 0, 'Initial load should preserve browser focus');
    const animationIds = () => page.evaluate(() => document.getAnimations().filter(a => a.id === 'feed-update').length);
    assert.equal(await animationIds(), 0, 'Initial load should be still');
    const update = (kind,id) => page.evaluate(async ([kind,id]) => {
      window.updateFeed(kind,id);
      await new Promise(requestAnimationFrame);
      return document.getAnimations().filter(animation => animation.id === 'feed-update').length;
    }, [kind,id]);
    const settle = () => page.evaluate(() => Promise.allSettled(document.getAnimations().map(animation => animation.finished)));
    for (const [kind,id] of [['insert','new'],['reorder','1'],['remove','new']]) {
      assert(await update(kind,id) > 0, 'Visible '+kind+' should move');
      await settle();
    }
    await update('insert','burst');
    await update('remove','burst');
    await settle();
    assert.equal(await page.locator('#posts article[data-post-id="burst"]').count(), 0);
    assert(await update('insert','motion-setting') > 0);
    await page.evaluate(() => document.getAnimations().filter(a => a.id === 'feed-update').forEach(a => a.pause()));
    await page.emulateMedia({reducedMotion:'reduce'});
    await page.waitForFunction(() => !document.getAnimations().some(a => a.id === 'feed-update'));
    await page.emulateMedia({reducedMotion:'no-preference'});
    const row = page.locator('#posts article[data-post-id="9"]');
    await row.evaluate(el => window.scrollBy(0, el.getBoundingClientRect().top - 68));
    const position = () => row.evaluate(el => el.getBoundingClientRect().top);
    const before = await position();
    const opener = row.getByRole('button', {name:'Post 9', exact:true});
    await opener.evaluate(el => el.focus({preventScroll:true}));
    await update('insert','above');
    assert(Math.abs(await position()-before) <= 1, 'Incoming post displaced the article during reading');
    await settle();
    assert(Math.abs(await position()-before) <= 1, 'Reading position changed after animation');
    assert(await opener.evaluate(el => el === document.activeElement), 'Incoming post took keyboard focus');
    await opener.press('Enter');
    await page.locator('#thread-heading:focus').waitFor();
    assert(await page.locator('#compose').isHidden(), 'Single-pane conversations have their own reply action');
    await update('insert','while-reading-thread');
    await page.getByRole('button',{name:'Inbox',exact:true}).click();
    await page.getByRole('button',{name:'Done',exact:true}).click();
    await page.locator('#thread-heading:focus').waitFor();
    await page.goBack();
    await opener.waitFor();
    assert(await opener.evaluate(el => el === document.activeElement), 'Back should restore the reader’s control');
    assert(Math.abs(await position()-before) <= 1, 'Back should restore the reading position');
    await page.emulateMedia({reducedMotion:'reduce'});
    assert.equal(await update('remove','above'), 0);
    assert(Math.abs(await position()-before) <= 1, 'Reduced Motion must preserve reading position');
    await page.evaluate(() => window.scrollTo(0,0));
    assert.equal(await update('insert','reduced'), 0, 'Reduce Motion should suppress visible insert animation');
    await page.setViewportSize({width:1280,height:840});
    await row.evaluate(el => window.scrollBy(0, el.getBoundingClientRect().top - 80));
    const bar = page.locator('.bar');
    await bar.evaluate(el => { el.style.minHeight = '100px'; });
    const rail = await page.locator('.app-nav').boundingBox();
    const header = await page.locator('.app-header').boundingBox();
    assert(rail.y >= header.y + header.height - 1, 'Sidebar crossed the header while scrolling');
    await bar.evaluate(el => el.style.removeProperty('min-height'));
    const desktop = await position();
    await update('insert','desktop');
    assert(Math.abs(await position()-desktop) <= 1, 'Desktop reading position changed');
    const removeMenu = row.getByRole('button',{name:'Post options'});
    await removeMenu.press('Enter');
    const menu = page.getByRole('menu');
    await menu.locator('[data-highlighted]').waitFor();
    await menu.press('h');
    await page.getByRole('menuitem', {name:'Hide on this device'}).and(page.locator('[data-highlighted]')).waitFor();
    await menu.press('Enter');
    await row.waitFor({state:'detached'});
    await page.locator('#read-10:focus').waitFor();
    const pinned = page.getByRole('navigation',{name:'Neighborhood'}).getByRole('button',{name:'Nearby'});
    await pinned.focus();
    await update('insert','while-browsing');
    assert(await pinned.evaluate(el => el === document.activeElement), 'An unrelated refresh took focus');
    await page.emulateMedia({reducedMotion:'no-preference'});
    assert.equal(await update('replace',''), 0, 'A replacement snapshot should render without a mass entrance');
    console.log('PASS live feed movement, stable reading position at 390px and 1280px, focus after Back/hide, and Reduce Motion.');
  } finally {
    await context.close();
  }
};
