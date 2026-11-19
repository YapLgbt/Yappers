const assert = require('node:assert/strict');

// A slow first visit should never offer a working-looking button before hydration.
module.exports = async function verifyDemo(browser, base) {
  const context = await browser.newContext({serviceWorkers: 'block'});
  let allowScripts;
  const gate = new Promise(resolve => { allowScripts = resolve; });
  try {
    const page = await context.newPage();
    await page.route('**/_app/immutable/**/*.js', async route => {
      await gate;
      await route.continue();
    });
    await page.goto(base + 'demo/', {waitUntil: 'commit'});
    await page.locator('#neighborhood[inert]').waitFor();
    const focused = await page.locator('#neighborhood [role=tab]').first().evaluate(tab => {
      tab.focus();
      return document.activeElement === tab;
    });
    assert.equal(focused, false, 'The prerendered controls must not accept focus');
    allowScripts();
    await page.locator('#neighborhood:not([inert])').waitFor();
    const card = page.locator('article').first();
    await card.getByRole('button', {name: 'Save', exact: true}).click();
    await card.getByRole('button', {name: 'Saved', exact: true}).waitFor();
    await page.getByRole('button', {name: 'Post', exact: true}).click();
    const editor = page.getByRole('dialog', {name: 'New post'});
    const anonymous = editor.getByRole('switch', {name: 'Anonymous mode'});
    await editor.locator('#public-byline strong').filter({hasText: /^You$/}).waitFor();
    assert.equal(await anonymous.isChecked(),false);
    assert.equal(await editor.getByRole('textbox',{name:'Display name',exact:true,includeHidden:true}).count(),0);
    await anonymous.focus();
    await page.keyboard.press('Space');
    assert(await anonymous.isChecked());
    await editor.locator('#public-byline strong').filter({hasText: /^Anonymous neighbor$/}).waitFor();
    await editor.getByLabel('Headline', {exact: true}).fill('Sunday book swap');
    await editor.getByLabel('Post', {exact: true}).fill('Bring a book to trade.');
    await editor.getByRole('button', {name: 'Post', exact: true}).click();
    const sample = page.locator('article').filter({has: page.getByRole('heading', {name: 'Sunday book swap'})});
    await sample.getByText('Anonymous neighbor', {exact:true}).waitFor();
    await sample.getByRole('button', {name: 'Replies (0)'}).click();
    await page.getByRole('button', {name: 'Write a reply'}).click();
    const reply = page.getByRole('dialog', {name: 'New reply'});
    const replyMode = reply.getByRole('switch', {name: 'Anonymous mode'});
    assert.equal(await replyMode.isChecked(),false);
    await reply.locator('#public-byline strong').filter({hasText: /^You$/}).waitFor();
    await reply.getByLabel('Reply', {exact: true}).fill('I’ll bring two.');
    await reply.getByRole('button', {name: 'Reply', exact: true}).click();
    const published = sample.locator('.reply:focus');
    await published.getByText('I’ll bring two.', {exact: true}).waitFor();
    await published.getByText('You', {exact:true}).waitFor();
    await page.goBack();
    await sample.locator('button[aria-label="Replies (1)"]:focus').waitFor();
    assert.equal(await page.evaluate(async () => (await import('/app/store.js')).load()), null);
    console.log('PASS local demo hydration, posting, replying, and focus on Back.');
  } finally {
    allowScripts();
    await context.close();
  }
};
