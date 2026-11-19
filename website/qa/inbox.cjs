const assert = require('node:assert/strict');
const {spawn} = require('node:child_process');
const {createInterface} = require('node:readline');
const path = require('node:path');

module.exports = async function verifyInbox(page, ctx, fixture, engine) {
  const peer = spawn('python3',[path.join(__dirname,'native-inbox.py'),fixture],{stdio:['pipe','pipe','inherit']});
  const lines = createInterface({input:peer.stdout})[Symbol.asyncIterator]();
  const read = async () => {
    const line = await lines.next();
    assert.ok(!line.done,'Native peer exited before completing an exchange');
    return JSON.parse(line.value);
  };
  async function native(command) {
    peer.stdin.write(JSON.stringify(command)+'\n');
    return read();
  }
  async function refresh() {
    await page.evaluate(async()=>{await (await import('/app/network.js')).sync()});
  }
  try {
    assert.equal(await read(),'ready');
    await refresh();
    await page.getByRole('button',{name:'Inbox',exact:true}).click();
    await page.getByText('No conversations yet').waitFor();
    await page.getByRole('tab',{name:'Conversations'}).focus();
    await page.keyboard.press('ArrowRight');
    await page.getByText('No message requests').waitFor();
    const receiver = await page.evaluate(async()=>{
      const profile=await (await import('/app/store.js')).load();
      return profile.wallet.chain.at(-1).payload.account;
    });
    if (engine === 'webkit') {
      await page.setViewportSize({width:390,height:844});
      await page.emulateMedia({colorScheme:'light'});
    }
    const first = 'Meet by the bookshelves?', reply = 'See you after the reading.';
    if (engine === 'webkit') {
      // Hold a native-initiated request until the person explicitly accepts it.
      await page.evaluate(async()=>{
        const {profile:p}=await import('/app/network.js');
        const {BrowserClient}=await import('/app/runtime/yap.js');
        const c=await BrowserClient.open(JSON.stringify({join:p.join,authorSecret:p.wallet.deviceSecret,
          nextSequence:p.wallet.nextSequence,accountChain:p.wallet.chain}));
        try {await c.request(JSON.stringify({type:'social',op:{kind:'setPrivacy',settings:{
          hideAnonymous:false,feedDays:0,dmDays:0,approvedOnly:true}}}));} finally {await c.close()}
      });
      await native({kind:'send',peer:receiver,body:reply});
      await refresh();
      assert.equal(await page.getByText(reply,{exact:true}).count(),0);
      await page.getByRole('button',{name:'Accept',exact:true}).click();
      await page.getByText(reply,{exact:true}).waitFor();
    } else {
      await page.getByRole('button',{name:'Nearby',exact:true}).click();
      const card = page.locator('article').filter({has:page.getByRole('heading',{name:'Native inbox test'})});
      assert.equal(await card.getByRole('button',{name:'Message',exact:true}).count(),0);
      await card.getByRole('button',{name:'Native reader',exact:true}).click();
      await page.getByRole('dialog').getByRole('button',{name:'Message Native reader',exact:true}).click();
    }
    await page.getByLabel('Message',{exact:true}).fill(first);
    await page.getByRole('button',{name:'Send',exact:true}).click();
    await page.getByText('Sent',{exact:true}).waitFor({timeout:65000});
    assert.deepEqual(await native({kind:'sync'}),[first]);
    if (engine !== 'webkit') {
      await native({kind:'send',peer:receiver,body:reply});
      await refresh();
      await page.getByText(reply,{exact:true}).waitFor();
    }
    await page.reload();
    await page.locator('#posts[data-feed-state=primary]').waitFor({timeout:65000});
    await page.getByRole('button',{name:'Inbox',exact:true}).click();
    await page.getByRole('button',{name:/Native reader/}).click();
    await page.getByText(reply,{exact:true}).waitFor();
    const originalViewport = page.viewportSize();
    try {
      for (const width of [390, 768]) {
        await page.setViewportSize({width,height:340});
        await page.getByLabel('Message options',{exact:true}).last().focus();
        await page.keyboard.press('Tab');
        const field = page.getByRole('textbox',{name:'Message',exact:true});
        assert(await field.evaluate(input => input === document.activeElement),
          `Tab should land on the Inbox composer at ${width}×340`);
        const visible = await field.evaluate(input => {
          const {left,right,top,bottom} = input.getBoundingClientRect();
          const center = (left + right) / 2;
          return {top,bottom,topHit:document.elementFromPoint(center,top+1)?.tagName,
            bottomHit:document.elementFromPoint(center,bottom-1)?.tagName,
            unobstructed:document.elementFromPoint(center,top+1) === input && document.elementFromPoint(center,bottom-1) === input};
        });
        assert(visible.unobstructed, `The focused Inbox composer should be visible at ${width}×340: ${JSON.stringify(visible)}`);
      }
    } finally {
      await page.setViewportSize(originalViewport);
    }
    if (process.env.YAP_QA_REPORTS === '1') {
      await page.getByLabel('Message options',{exact:true}).click();
      await page.getByRole('menuitem',{name:'Report message',exact:true}).click();
      const dialog=page.getByRole('dialog',{name:'Report to your home'});
      const evidence=dialog.getByLabel('Evidence for the moderator');
      assert.equal(await evidence.inputValue(),'');
      assert.equal(await dialog.getByLabel('Include account ID').isChecked(),false);
      assert.equal(await dialog.getByRole('button',{name:'Review report'}).isDisabled(),true);
      await dialog.getByRole('button',{name:'Use this message’s text'}).click();
      assert.equal(await evidence.inputValue(),reply);
      await evidence.fill('after the reading');
      await dialog.getByRole('button',{name:'Review report'}).click();
      await dialog.getByText('after the reading',{exact:true}).waitFor();
      assert.equal(await dialog.getByText(reply,{exact:true}).count(),0);
      await dialog.getByRole('button',{name:'Done'}).click();
    }
    if(process.env.YAP_QA_SCREENSHOTS) await page.screenshot({path:path.join(process.env.YAP_QA_SCREENSHOTS,'inbox-'+engine+'.png')});
    // Queue against the established ratchet offline, then deliver exactly once across tabs.
    await ctx.setOffline(true);
    await page.getByLabel('Message',{exact:true}).fill('Saved offline');
    await page.getByRole('button',{name:'Send',exact:true}).click();
    await page.getByText('Waiting to send',{exact:true}).waitFor();
    await ctx.setOffline(false);
    const other=await ctx.newPage();
    try {
      await other.goto(page.url());
      await other.locator('#posts[data-feed-state=primary]').waitFor({timeout:65000});
      await refresh();
      assert.deepEqual(await native({kind:'sync'}),[first,'Saved offline']);
    } finally {await other.close()}
    await ctx.setOffline(true);
    await page.getByLabel('Message',{exact:true}).fill('Cancel on block');
    await page.getByRole('button',{name:'Send',exact:true}).click();
    await page.getByText('Waiting to send',{exact:true}).waitFor();
    page.once('dialog',dialog=>dialog.accept());
    await page.getByRole('button',{name:'Block',exact:true}).click();
    await page.getByText('No conversations yet').waitFor();
    await ctx.setOffline(false);
    await page.getByRole('button',{name:'Nearby',exact:true}).click();
    await refresh();
    assert.equal(await page.getByRole('heading',{name:'Native inbox test'}).count(),0);
    const cancelled = await page.evaluate(async()=>{
      const state=await (await import('/app/store.js')).load();
      return state.directState.messages.find(message=>message.body==='Cancel on block')?.status === 'unavailable' &&
        state.directState.outbox.length === 0;
    });
    assert.ok(cancelled,'Blocking must cancel a previously queued message');
    assert.deepEqual(await native({kind:'sync'}),[first,'Saved offline']);
    console.log('PASS bidirectional native/Web encrypted DMs, offline retry, cross-tab persistence and blocking ('+engine+').');
  } finally {peer.stdin.end(); peer.kill();}
};
