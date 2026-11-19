const assert = require('node:assert/strict');
const {spawn} = require('node:child_process');
const {createInterface} = require('node:readline');
const path = require('node:path');

module.exports = async function verifyInbox(page, ctx, fixture, engine) {
  const photo = await page.evaluate(() => {
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 96;
    const ctx = canvas.getContext('2d');
    ctx.fillStyle = '#20bf60';
    ctx.fillRect(0, 0, 96, 96);
    return canvas.toDataURL('image/jpeg').split(',')[1];
  });
  async function expectContactPhoto(row) {
    const canvas = row.locator('.avatar canvas');
    await canvas.waitFor({state:'visible'});
    const [r, g, b] = await canvas.evaluate(c => [...c.getContext('2d').getImageData(48,48,1,1).data]);
    assert(g > 175 && r < 50 && b < 110, 'Inbox should show the signed contact photo');
    assert.equal(await row.locator('.avatar img').count(), 0);
  }
  const peer = spawn('python3',[path.join(__dirname,'native-inbox.py'),fixture,photo],{stdio:['pipe','pipe','inherit']});
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
    await page.getByRole('button',{name:/^Inbox(?:,|$)/}).click();
    await page.getByText('No conversations yet').waitFor();
    await page.getByRole('tab',{name:'Messages'}).focus();
    await page.keyboard.press('ArrowLeft');
    await page.getByText('No replies yet').waitFor();
    const parent = await page.evaluate(async () => {
      const home = await import('/app/network.js');
      const id = await home.queue({kind:'postV2', anonymous:true, displayName:'Anonymous neighbor',
        topic:'Chatter', headline:'Inbox activity test', body:'Looking for a book'});
      await home.sync();
      return id;
    });
    if (engine==='chrome' && process.env.YAP_TEST_PUSH_ROUTE) {
      await page.evaluate(async()=>{
        const {profile:p}=await import('/app/network.js');
        const {BrowserClient}=await import('/app/runtime/yap.js');
        const client=await BrowserClient.open(JSON.stringify({join:p.join,authorSecret:p.wallet.deviceSecret,
          nextSequence:p.wallet.nextSequence,accountChain:p.wallet.chain}));
        try { await client.request(JSON.stringify({type:'social',op:{kind:'setPush',provider:'fcm',
          subscription:crypto.randomUUID(),categories:['reply']}})); } finally {await client.close();}
      });
    }
    await native({kind:'reply',parent});
    await refresh();
    const activity=page.getByRole('button',{name:/Inbox activity test.*1 new reply/});
    await activity.click();
    await page.locator('.reply').filter({hasText:'Following up on your post'}).waitFor();
    await page.goBack();
    await refresh();
    await page.getByRole('tab',{name:'Activity'}).click();
    const opened=page.getByRole('button',{name:/Native reader replied.*Following up on your post.*Inbox activity test/});
    await opened.waitFor();
    assert.equal(await opened.locator('.new-count').count(),0);
    await expectContactPhoto(opened);
    assert.match(await opened.locator('time').innerText(),/^(?:Just now|\d+m)$/);
    if (process.env.YAP_QA_SCREENSHOTS) {
      const originalSize=page.viewportSize();
      const original=await page.evaluate(async()=>{
        const home=await import('/app/network.js');
        const original=home.profile.notifications.activity;
        const now=Math.floor(Date.now()/1000);
        const examples=[
          ['Mira','Sunday book swap','I’ll bring The Dispossessed. Are we still meeting by the library steps?',300,2],
          ['Anonymous neighbor','First time at skate night','Same here! Happy to meet at the entrance and head in together.',3600,1],
          ['Jules','Where do you buy zines?','The shop on Maple has a whole shelf of local artists.',20*86400,0],
          ['Robin','Picnic in the park','The west entrance is step-free.',61*86400,0],
          ['Ash','Community garden day','I have a spare pair of gloves you can borrow.',380*86400,0]
        ];
        await home.modify(s=>({...s,notifications:{...s.notifications,activity:examples.map(([byline,title,preview,age,unread],i)=>({
          ...original[0],target:'preview-'+i,byline,title,preview,unread,anonymous:i===1,photo:i===1?null:'',created:now-age
        }))}}));
        return original;
      });
      try {
        await page.setViewportSize({width:390,height:844});
        const rows=page.locator('.activity-row');
        await rows.filter({hasText:'Sunday book swap'}).waitFor();
        assert.deepEqual(await rows.locator('time').allTextContents(),['5m','1h','3w','2mo','1y']);
        await page.screenshot({path:path.join(process.env.YAP_QA_SCREENSHOTS,'activity-'+engine+'.png')});
      } finally {
        await page.evaluate(async original=>{
          const home=await import('/app/network.js');
          await home.modify(s=>({...s,notifications:{...s.notifications,activity:original}}));
        },original);
        await page.setViewportSize(originalSize);
      }
    }
    if (engine==='chrome' && process.env.YAP_TEST_PUSH_ROUTE) {
      // The recording transport supplies the same opaque payload received from a provider.
      await native({kind:'reply',parent});
      let payload;
      const fs=require('node:fs/promises');
      for(let i=0;i<100 && !payload;i++) {
        try {payload=JSON.parse(await fs.readFile(process.env.YAP_TEST_PUSH_ROUTE,'utf8'));} catch {}
        if(!payload) await new Promise(resolve=>setTimeout(resolve,150));
      }
      assert.match(payload?.route||'',/^[a-f0-9]{64}$/);
      const url=new URL(page.url());
      await ctx.grantPermissions(['notifications'],{origin:url.origin});
      const cdp=await ctx.newCDPSession(page);
      const registration=new Promise(resolve=>cdp.on('ServiceWorker.workerRegistrationUpdated',({registrations})=>{
        const app=registrations.find(reg=>reg.scopeURL===url.origin+'/app/'); if(app) resolve(app.registrationId);
      }));
      await cdp.send('ServiceWorker.enable');
      await cdp.send('ServiceWorker.deliverPushMessage',{origin:url.origin,registrationId:await registration,data:JSON.stringify(payload)});
      await page.waitForFunction(async route=>{
        const reg=await navigator.serviceWorker.ready;
        return (await reg.getNotifications({tag:route})).some(n=>n.title==='Yap' && n.body==='You have an Inbox update.');
      },payload.route);
      await page.goto('about:blank');
      await page.goto(url.origin+'/app/#inbox='+payload.route);
      await page.locator('#thread-pane .reply').filter({hasText:'Following up on your post'}).first().waitFor({timeout:65000});
      await page.goBack();
      await page.getByRole('tab',{name:'Messages'}).waitFor();
      console.log('PASS worker displayed a generic alert and cold launch opened its authenticated reply thread.');
    }
    console.log('PASS Inbox activity opens the reply and preserves read progress ('+engine+').');
    await page.getByRole('tab',{name:'Messages'}).click();
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
      await page.getByRole('button',{name:/Message requests/}).click();
      await expectContactPhoto(page.locator('.dm-request'));
      await page.getByRole('button',{name:'Accept',exact:true}).click();
      await page.getByText(reply,{exact:true}).waitFor();
    } else {
      await page.getByRole('button',{name:'Nearby',exact:true}).click();
      const card = page.locator('article').filter({has:page.getByRole('heading',{name:'Native inbox test'})});
      assert.equal(await card.getByRole('button',{name:'Message',exact:true}).count(),0);
      await card.getByRole('button',{name:'Native reader',exact:true}).click();
      await page.getByRole('dialog').getByRole('button',{name:'Message Native reader',exact:true}).click();
      await page.getByRole('button',{name:'Back to inbox',exact:true}).click();
      await page.getByRole('button',{name:'Done',exact:true}).click();
      await card.waitFor();
      await page.goForward();
      await page.getByRole('tab',{name:'Messages'}).waitFor();
      await page.goForward();
    }
    await expectContactPhoto(page.locator('.recipient'));
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
    await page.getByRole('button',{name:/^Inbox(?:,|$)/}).click();
    await expectContactPhoto(page.getByRole('button',{name:/Native reader/}));
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
