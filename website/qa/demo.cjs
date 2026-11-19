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
    assert.equal(await page.evaluate(async () => (await import('/app/store.js')).load()), null);
    console.log('PASS demo controls become interactive on hydration.');
  } finally {
    allowScripts();
    await context.close();
  }
};
