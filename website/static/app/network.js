import init, {BrowserClient, create_wallet, prepare_action, recover_browser_wallet} from './runtime/yap.js';
import {load, update, clearDevice} from './store.js';

let client;
export let profile;
export let replicaFeed;
const channel = new BroadcastChannel('yap-web-updates');
export const updates = new EventTarget();
function changed() { updates.dispatchEvent(new Event('change')); }
channel.onmessage = async () => {
  const latest = await load();
  if (profile && latest?.wallet.deviceSecret !== profile.wallet.deviceSecret) {
    globalThis.location.reload?.();
    return;
  }
  profile = latest;
  changed();
};
export async function modify(change) {
  profile = await update(state => {
    if (activeDevice && state?.wallet.deviceSecret !== activeDevice) throw new Error('Your account changed. Reopen Yap.');
    return change(state);
  });
  channel.postMessage('changed');
  changed();
}
async function discover() {
  const response = await fetch('/.well-known/yap-home.json', {cache:'no-store', signal:AbortSignal.timeout(10000)});
  if (!response.ok) throw new Error('Home discovery is unavailable.');
  const manifest = await response.json();
  const join = manifest.join;
  const id = join?.serverAddr?.id;
  if (manifest.version !== 1 || !/^[a-f0-9]{64}$/.test(id) || join.community !== id + '/public' ||
      join.networkMode !== 'n0' || typeof join.accessToken !== 'string') throw new Error('Invalid public enrollment.');
  return manifest;
}
let activeDevice;
let activeRoute;
let nextDiscovery = 0;
async function openClient(state) {
  return BrowserClient.open(JSON.stringify({
    join:state.join, authorSecret:state.wallet.deviceSecret, nextSequence:state.wallet.nextSequence,
    accountChain:state.wallet.chain, ...(state.fullHomeStandby && {fullHomeStandby:state.fullHomeStandby})
  }));
}
async function applyEnrollment(enrollment) {
  if (profile.join.serverAddr.id !== enrollment.join.serverAddr.id) return;
  const route = {join:enrollment.join, replica:enrollment.publicFeedReplica || null,
    fullHomeStandby:enrollment.fullHomeStandby || null};
  const key = JSON.stringify(route);
  if (key === activeRoute) return;
  // Rust authenticates the owner's signature before the route is persisted or used.
  const device = profile.wallet.deviceSecret;
  const replacement = await openClient({...profile,...route});
  try {
    await modify(state => {
      if (state.wallet.deviceSecret !== device) throw new Error('The browser account changed.');
      return {...state,...route};
    });
  } catch (error) {
    await replacement.close();
    throw error;
  }
  const previous = client;
  activeDevice = device;
  client = replacement;
  activeRoute = key;
  if (previous) await previous.close();
}
export async function start(intent, alias = '', invitation = '') {
  alias = alias.trim();
  if (intent === 'create' && (alias.length < 2 || alias.length > 30))
    throw new Error('Choose a display name between 2 and 30 characters.');
  if (!alias) alias = 'Neighbor';
  if (!navigator.locks || !crypto.subtle) throw new Error('Open Yap in a browser with secure device storage.');
  await init();
  profile = await load();
  if (!intent && (!profile || profile.welcomePending && !profile.pendingRecovery)) return false;
  if (profile) changed();
  let enrollment;
  if (invitation.trim()) {
    if (profile && (!profile.welcomePending || hasLocalActivity(profile) || profile.pendingRecovery || profile.pendingTransfer))
      throw new Error('This browser already has an account. Use Account data to move homes.');
    let join;
    try {join=JSON.parse(invitation)} catch {throw new Error('Check your invitation and try again.')}
    if(!join?.serverAddr?.id || !join.community || !join.accessToken)throw new Error('Choose a home invitation from its owner.');
    const wallet=JSON.parse(create_wallet(join.serverAddr.id));
    const probe=await openClient({wallet,join});
    try{
      const feed=JSON.parse(await probe.request(JSON.stringify({type:'feed'})));
      if(feed.type!=='feed')throw new Error('Home unavailable');
    }catch{throw new Error('Couldn’t join this home. Check the invitation and your connection.')}
    finally{await probe.close()}
    await modify(s=>{
      if(s && (hasLocalActivity(s)||!s.welcomePending||s.pendingRecovery||s.pendingTransfer))throw new Error('This browser account changed. Reopen Yap.');
      return {wallet,join,posts:[],outbox:[],saved:[],blocks:[],pendingUnblocks:[],hideAnonymous:false,alias,safeForPublic:false,setupStep:'username'};
    });
    if(client)await client.close();
    client=null;activeDevice=null;activeRoute=null;
  }else{
    try { enrollment = await discover(); }
    catch (error) { if (!profile) throw error; }
  }
  if (!profile) {
    await modify(old => old || {join:enrollment.join, wallet:JSON.parse(create_wallet(enrollment.join.serverAddr.id)),
      posts:[], outbox:[], saved:[], blocks:[], pendingUnblocks:[], hideAnonymous:false,
      alias, safeForPublic:true, setupStep:intent === 'create' ? 'username' : null, welcomePending:intent === 'restore'});
  } else if (intent === 'create' && profile.welcomePending) {
    await modify(s => {
      if (s.pendingRecovery) throw new Error('Finish restoring with your original recovery key or file.');
      return {...s,alias,welcomePending:false,setupStep:'username'};
    });
  }
  if (enrollment) {
    try { await applyEnrollment(enrollment); } catch { /* Use the previously pinned route. */ }
  }
  if (!client) {
    client = await openClient(profile);
    activeDevice = profile.wallet.deviceSecret;
  }
  nextDiscovery = Date.now() + 300000;
  if (!intent && !profile.pendingTransfer && profile.pendingRecovery && navigator.onLine) {
    const saved = profile.pendingRecovery;
    const {account, home} = saved.chain.at(-1).payload;
    try {
      await restoreIdentity({account, home, root:saved.rootSecret});
      location.reload();
    } catch { /* The same user-held file can retry an interrupted registration. */ }
  }
  return !profile.welcomePending;
}
async function rpc(command) {
  return JSON.parse(await client.request(JSON.stringify(command)));
}
export const reportDraft = () => rpc({type:'reportDraft'});
export const reportStatus = receipt => rpc({type:'reportStatus',receipt});
export const submitReport = (receipt,evidence) => rpc({type:'submitReport',receipt,evidence});
export function usernameDomain(state) {
  return state?.usernameDomain || (state?.join.serverAddr.id === '114f92b50e66dd84a14e0ff9b857b0c254565609b00b0cb9ef85f29e864efe5b' ? 'yap.lgbt' : null);
}
// Account setup trusts the primary home's authenticated privacy status, including an explicit empty claim.
export async function verifyWelcomeUsername(finish = false) {
  return navigator.locks.request('yap-web-delivery-v1', async () => {
    const wallet = await confirmAccount();
    const feed = await rpc({type:'feed'});
    const status = feed.privacy === true ? await rpc({type:'social',op:{kind:'privacyStatus'}}) : null;
    const domain = status?.usernameDomain;
    if (typeof domain !== 'string' || !domain.trim())
      throw new Error('This home does not support usernames. Ask its operator to enable them, then try again.');
    const username = status.username ?? null;
    if (username !== null && (typeof username !== 'string' || !/^[a-z][a-z0-9_]{1,23}$/.test(username.split('@')[0]) ||
        username !== username.split('@')[0] + '@' + domain))
      throw new Error('Your home returned an invalid username. Try again.');
    if (finish && !username) throw new Error('Claim a username before entering the neighborhood.');
    await modify(s => {
      if (s.wallet.deviceSecret !== wallet.deviceSecret) throw new Error('Your account changed. Reopen Yap.');
      return {...s,usernameDomain:domain,username,setupStep:finish ? null : s.setupStep};
    });
    return {domain,username};
  });
}
export async function claimUsername(name) {
  await navigator.locks.request('yap-web-delivery-v1', async () => {
    const wallet = await confirmAccount();
    const domain = usernameDomain(profile);
    if (!domain || !/^[a-z][a-z0-9_]{1,23}$/.test(name)) throw new Error('Enter a valid username for this home.');
    const result = await rpc({type:'social',op:{kind:'claimUsername',name}});
    if (result.username !== name + '@' + domain) throw new Error('Your home did not confirm this username. Try again.');
    await modify(s=>{
      if (s.wallet.deviceSecret !== wallet.deviceSecret) throw new Error('Your account changed. Reopen Yap.');
      return {...s,username:result.username};
    });
  });
}
function viewKey(state) {
  const scope = state.activeGroup || state.join.community;
  const area = !state.activeGroup && state.nearby;
  return JSON.stringify([scope, area ? [area.center.lat100, area.center.lon100, area.radiusMiles] : null]);
}
function milesBetween(a, b) {
  const rad = Math.PI / 18000;
  const h = Math.sin((b.lat100-a.lat100)*rad/2)**2 + Math.cos(a.lat100*rad)*Math.cos(b.lat100*rad)*Math.sin((b.lon100-a.lon100)*rad/2)**2;
  return 7917.52 * Math.asin(Math.sqrt(Math.min(1,h)));
}
export function queuedPosts(state, feed) {
  const scope = state.activeGroup || state.join.community;
  const area = !state.activeGroup && state.nearby;
  const known = new Set(feed.map(post => post.id));
  return state.outbox.flatMap(({id, payload}) => {
    const action = payload.action;
    if (known.has(id) || payload.community !== scope || !['post', 'postV2'].includes(action.kind) ||
      (area && (!action.area || milesBetween(action.area, area.center) > area.radiusMiles))) return [];
    return [{
      id, scope, pending: true, mine: true, author: action.displayName, visibility: { anonymous: action.anonymous },
      username: action.anonymous ? undefined : state.username, profile: action.profile, area: action.area,
      headline: action.headline, body: action.body, topic: action.topic, createdAt: payload.createdAt, replies: []
    }];
  }).reverse();
}
function changeFeed(state, change) {
  const next = {...state,...change};
  if (viewKey(state) === viewKey(next)) return next;
  const history = {...state.feedCache};
  if (state.lastSync) {
    delete history[viewKey(state)];
    history[viewKey(state)] = {posts:state.posts, lastSync:state.lastSync};
  }
  const feedCache = Object.fromEntries(Object.entries(history).slice(-8));
  let cached = feedCache[viewKey(next)];
  if (!cached && !next.activeGroup && next.nearby) {
    const all = feedCache[viewKey({...next,nearby:null})];
    if (all) cached = {...all, posts:all.posts.filter(p=>p.area && milesBetween(p.area,next.nearby.center)<=next.nearby.radiusMiles)};
  }
  return {...next,feedCache,posts:cached?.posts || [],lastSync:cached?.lastSync || null};
}
function purgeDeleted(state, learned = []) {
  const known = new Set([...(state.deleted || []), ...learned]);
  if (!known.size) return state;
  const savedPosts=Object.fromEntries(Object.entries(state.savedPosts||{}).filter(([id])=>!known.has(id))
    .map(([id,p])=>[id,{...p,replies:p.replies.filter(reply=>!known.has(reply.id))}]));
  const feedCache = Object.fromEntries(Object.entries(state.feedCache || {}).map(([key,entry])=>[key,{...entry,
    posts:entry.posts.filter(p=>!known.has(p.id)).map(p=>({...p,replies:p.replies.filter(r=>!known.has(r.id))}))}]));
  return {...state,savedPosts,feedCache,ownedEvents:state.ownedEvents?.filter(r=>!known.has(r.event.id)||r.deletion),
    recoveredDrafts:state.recoveredDrafts?.filter(d=>!known.has(d.id)),deleted:[...known], posts:state.posts.filter(post=>!known.has(post.id))
    .map(post=>({...post,replies:post.replies.filter(reply=>!known.has(reply.id))})),
    outbox:state.outbox.filter(event=>!known.has(event.id)), saved:state.saved.filter(id=>!known.has(id))};
}
export async function queue(action, _legacyAlias, scope = profile.activeGroup || profile.join.community) {
  let eventId;
  await modify(state => {
    if (state.setupStep || state.welcomePending) throw new Error('Finish setting up your account before posting.');
    if (!state.username && (action.anonymous === false || action.kind === 'reply' && action.displayName !== 'Anonymous neighbor'))
      throw new Error('Choose a username before posting by name.');
    if (state.pendingRecovery || state.pendingTransfer) throw new Error('Finish restoring your account before posting.');
    if (state.outbox.length >= 32) throw new Error('Your outbox is full. Sync before adding another post.');
    const target = action.target || action.parent;
    const savedTarget = target && Object.values(state.savedPosts||{}).some(p=>p.scope===scope && (p.id===target || p.replies.some(r=>r.id===target)));
    if (scope !== (state.activeGroup || state.join.community) && !savedTarget) throw new Error('The feed changed. Reopen the composer.');
    const prepared = JSON.parse(prepare_action(JSON.stringify(state.wallet),scope,JSON.stringify(action)));
    eventId = prepared.event.id;
    return {...state, wallet:{...state.wallet,nextSequence:prepared.nextSequence},
      outbox:[...state.outbox,prepared.event]};
  });
  scheduleDelivery();
  return eventId;
}
export function canShowPublic(state) {
  return !state.activeGroup && !state.nearby && !state.pendingPrivacy && !state.hideAnonymous && !state.inboxPolicy?.feedDays && state.safeForPublic && !state.blocks.length && !state.pendingUnblocks.length;
}
let activeSync;
export function sync() {
  if (!activeSync) activeSync = synchronize().finally(() => { activeSync = undefined; });
  return activeSync;
}
async function synchronize() {
  if (!client) throw new Error('Connect to your home first.');
  if (!navigator.onLine) throw new Error('Offline');
  const getFeed = () => scopedRpc(profile.activeGroup, profile.nearby && !profile.activeGroup
    ? {type:'nearbyFeed',nearby:profile.nearby}
    : !profile.fullHomeStandby && profile.replica && canShowPublic(profile)
      ? {type:'feedWithFailover',replica:profile.replica} : {type:'feed'});
  // A separate lock serializes network delivery across tabs. UI changes only need the shorter device lock.
  return navigator.locks.request('yap-web-delivery-v1', async () => {
    profile = await load();
    if (activeDevice !== profile?.wallet.deviceSecret) {
      globalThis.location.reload?.();
      throw new Error('Reopening the active account.');
    }
    if (Date.now() >= nextDiscovery) {
      nextDiscovery = Date.now() + 300000;
      try { await applyEnrollment(await discover()); } catch { /* Retry discovery on the next interval. */ }
    }
    if (profile.pendingTransfer || profile.pendingRecovery) throw new Error('Finish importing your backup.');
    if (profile.setupStep || profile.welcomePending) throw new Error('Finish setting up your account before syncing.');
    if (profile.moveNotice) {
      const notice=profile.moveNotice;
      try {await rpc({type:'accountNotify',...notice});await modify(s=>({...s,moveNotice:null}));} catch { /* Retry after connecting again. */ }
    }
    if (profile.pendingPrivacy) {
      const settings = profile.pendingPrivacy;
      await confirmAccount();
      await rpc({type:'social',op:{kind:'setPrivacy',settings}});
      await modify(s => JSON.stringify(s.pendingPrivacy) === JSON.stringify(settings) ? {...s,pendingPrivacy:null} : s);
    }
    const feed = await getFeed();
    if (feed.type === 'publicReplicaFeed') {
      await modify(s=>purgeDeleted(s,feed.deleted));
      replicaFeed = {...feed,posts:purgeDeleted({...profile,posts:feed.posts}).posts};
      profile = await load();
      changed();
      return 'replica';
    }
    if (feed.type !== 'feed') throw new Error('Unexpected home response.');
    replicaFeed = undefined;
    await modify(s=>purgeDeleted(s,feed.deleted));
    const pendingUnblocks = profile.pendingUnblocks;
    for (const id of pendingUnblocks) {
      await rpc({type:'social',op:{kind:'unblock',id}});
      await modify(s=>({...s,pendingUnblocks:s.pendingUnblocks.filter(v=>v!==id)}));
    }
    const pendingBlocks = profile.blocks.filter(b=>!b.handle);
    for (const block of pendingBlocks) {
      const result = await scopedRpc(block.scope,{type:'social',op:{kind:'block',target:block.byAccount ? {account:block.peer} : {event:block.event}}});
      if (!result.id) throw new Error('Block is waiting to sync.');
      await modify(s=>({...s,posts:[],savedPosts:{},feedCache:{},blocks:s.blocks.map(b=>b.event===block.event?{...b,handle:result.id}:b),
        pendingUnblocks:s.blocks.some(b=>b.event===block.event)?s.pendingUnblocks:[...s.pendingUnblocks,result.id]}));
    }
    for (const peer of profile.pendingApprovals || []) {
      await rpc({type:'social',op:{kind:'approve',peer}});
      await modify(s=>({...s,pendingApprovals:s.pendingApprovals.filter(id=>id!==peer)}));
    }
    let sent = 0;
    const acceptedFeedEvents = new Set();
    for (const event of profile.outbox) {
      const receipt=await scopedRpc(event.payload.community,{type:'submit',event});
      const removed = event.payload.action.kind==='delete' ? [event.payload.action.target] : receipt.envelope?.deletion ? [event.id] : [];
      // Keep the local article or reply until the same store update supplies its confirmed version.
      const awaitingFeed = ['post', 'postV2', 'reply'].includes(event.payload.action.kind) && !removed.length;
      if (awaitingFeed) acceptedFeedEvents.add(event.id);
      await modify(s=>purgeDeleted({...s,posts:event.payload.action.kind==='vote'?s.posts.map(post=>post.id===event.payload.action.target
        ? {...post,vote:event.payload.action.value,score:post.score-post.vote+event.payload.action.value}:post):s.posts,ownedEvents:mergeHistory(s.ownedEvents,[receipt.erased,receipt.envelope].filter(Boolean)),outbox:awaitingFeed ? s.outbox : s.outbox.filter(v=>v.id!==event.id)},removed));
      if(event.payload.community!==(profile.activeGroup||profile.join.community) && [event.payload.action.parent,event.payload.action.target].some(id=>id && profile.savedPosts?.[id])){
        const other=await scopedRpc(event.payload.community,{type:'feed'});
        await modify(s=>{
          const savedPosts={...s.savedPosts};
          for(const post of other.posts)if(s.saved.includes(post.id))savedPosts[post.id]={...post,scope:event.payload.community};
          return purgeDeleted({...s,savedPosts},other.deleted);
        });
      }
      sent++;
    }
    // Refresh after each delivery batch before publishing the personalized projection.
    const latest = pendingBlocks.length || pendingUnblocks.length || sent ? await getFeed() : feed;
    const [blocks, privacy] = await Promise.all([
      rpc({type:'social',op:{kind:'blocks'}}), rpc({type:'social',op:{kind:'privacyStatus'}})
    ]);
    const safe = !blocks.blocks.length && !privacy.settings.hideAnonymous && !privacy.settings.feedDays;
    if (latest.accountChain?.length && latest.accountChain.at(-1).payload.account !== profile.wallet.chain.at(-1).payload.account) {
      throw new Error('Your home returned a different account.');
    }
    const confirmedBlocks=blocks.blocks.map(b=>profile.blocks.find(local=>local.handle===b.id) || {event:'home-'+b.id,handle:b.id,label:b.label});
    const newlyBlocked=confirmedBlocks.some(b=>!profile.blocks.some(old=>old.handle===b.handle));
    await modify(s => {
      const savedPosts=newlyBlocked?{}:{...s.savedPosts};
      const scope=s.activeGroup||s.join.community;
      for(const post of latest.posts)if(s.saved.includes(post.id))savedPosts[post.id]={...post,scope};
      return purgeDeleted({...s,savedPosts,outbox:s.outbox.filter(event=>!acceptedFeedEvents.has(event.id)),feedCache:newlyBlocked ? {} : s.feedCache,blocks:[...confirmedBlocks,...s.blocks.filter(b=>!b.handle)], posts:latest.posts, lastSync:Date.now(), safeForPublic:safe,
      directAvailable:latest.direct === true && !s.mailboxElsewhere, messageRequests:privacy.requests || [], inboxPolicy:s.pendingPrivacy || privacy.settings, hideAnonymous:s.pendingPrivacy?.hideAnonymous ?? (s.privacyManaged ? privacy.settings.hideAnonymous : s.hideAnonymous || privacy.settings.hideAnonymous),
      usernameDomain:privacy.usernameDomain || s.usernameDomain, username:privacy.username || s.username,
      wallet:latest.accountChain?.length ? {...s.wallet,chain:latest.accountChain} : s.wallet},latest.deleted);});
    if (latest.direct === true) {
      try {
        await initializeMessages();
        // Commit each receive cursor and prepared packet before the next network round.
        do {
          await messageStep({kind:'sync', blocked:profile.blocks.map(block => block.peer).filter(Boolean)});
        } while (profile.directState.outbox.length && !profile.directState.delivery_paused);
        if (profile.mailboxElsewhere) await modify(s => ({...s,mailboxElsewhere:false,directAvailable:true}));
        inboxError = '';
      } catch (error) {
        if (isMailboxMismatch(error)) {
          await modify(s => ({...s,mailboxElsewhere:true,directAvailable:false}));
          inboxError = otherMessagingDevice;
        } else inboxError = 'Couldn’t update your inbox. Refresh to try again.';
      }
      changed();
    }
    return 'primary';
  });
}

export let inboxError = '';
export const otherMessagingDevice = 'This account’s inbox is linked to another installation of Yap. Open it there to read or send messages. Account recovery doesn’t transfer inbox keys.';
function isMailboxMismatch(error) {
  return String(error).split(': ').at(-1) === 'messaging is registered to a different device';
}

async function messageStep(action) {
  const device = profile.wallet.deviceSecret;
  const result = await rpc({type:'direct', state:profile.directState ?? null, action});
  await modify(state => {
    if (state.wallet.deviceSecret !== device) throw new Error('Your account changed. Reopen Yap.');
    return {...state, directState:result.state, ...(action.kind === 'prepare' && {
      revokedContacts:(state.revokedContacts || []).filter(id => id !== action.peer),
      approvedContacts:[...new Set([...(state.approvedContacts || []),action.peer])]
    })};
  });
}

async function initializeMessages() {
  if (!profile.directState) await messageStep({kind:'initialize'});
}

function withMailbox(task) {
  return navigator.locks.request('yap-web-delivery-v1', async () => {
    profile = await load();
    if (!profile || !client || activeDevice !== profile.wallet.deviceSecret || profile.pendingRecovery || profile.pendingTransfer)
      throw new Error('Reopen Yap to use this account.');
    if (profile.setupStep || profile.welcomePending) throw new Error('Finish setting up your account before messaging.');
    if (profile.mailboxElsewhere) throw new Error(otherMessagingDevice);
    if (!profile.directAvailable) throw new Error('Connect to your home to use messages.');
    try { return await task(); }
    catch (error) {
      if (!isMailboxMismatch(error)) throw error;
      await modify(s => ({...s,mailboxElsewhere:true,directAvailable:false}));
      throw new Error(otherMessagingDevice);
    }
  });
}

function checkPeer(peer) {
  if (profile.blocks.some(block => block.peer === peer)) throw new Error('Unblock this account first.');
}

export function messageContact(event,scope) {
  return withMailbox(async () => {
    const contact = await scopedRpc(scope||profile.activeGroup,{type:'social',op:{kind:'contact',event}});
    checkPeer(contact.peer);
    return contact;
  });
}

export function sendMessage(contact, body) {
  return withMailbox(async () => {
    checkPeer(contact.peer);
    await initializeMessages();
    await messageStep({kind:'prepare',peer:contact.peer,label:contact.label,name:profile.alias,body:body.trim()});
    scheduleDelivery();
  });
}

// Keep a local block and cancellation of still-queued packets in the same encrypted write.
// The send ratchet stays advanced. Unblocking must never resurrect an old ciphertext.
export async function blockAccount(block) {
  await navigator.locks.request('yap-web-delivery-v1', () => modify(state => {
    if (!state || state.wallet.deviceSecret !== activeDevice) throw new Error('Reopen Yap to use this account.');
    if (state.blocks.some(item => item.event === block.event || (block.peer && item.peer === block.peer))) return state;
    const direct = state.directState;
    const peer = block.peer;
    return {...state,savedPosts:{},feedCache:{}, blocks:[...state.blocks,{...block,scope:block.scope || state.activeGroup || state.join.community}],
      pendingApprovals:(state.pendingApprovals || []).filter(id=>id!==peer),
      approvedContacts:(state.approvedContacts || []).filter(id => id !== peer),
      revokedContacts:peer ? [...new Set([...(state.revokedContacts || []),peer])] : state.revokedContacts,
      directState:direct && peer ? {...direct,
        outbox:direct.outbox.filter(packet => packet.value.to !== peer),
        messages:direct.messages.map(message => message.peer === peer && message.status === 'queued'
          ? {...message,status:'unavailable'} : message)} : direct};
  }));
  scheduleDelivery();
}

export function unblockAccount(account) {
  return accountTask(async()=>{
    await modify(state=>{
      const block=state.blocks.find(b=>b.event===account.event);
      if (!block) return state;
      return {...state,blocks:state.blocks.filter(b=>b!==block),
        pendingUnblocks:[...new Set([...state.pendingUnblocks,...(block.handle?[block.handle]:[])])]};
    });
    scheduleDelivery();
  });
}

export function approveMessage(peer) {
  return withMailbox(async () => {
    checkPeer(peer);
    await modify(state => ({...state,
      pendingApprovals:[...new Set([...(state.pendingApprovals || []),peer])],
      approvedContacts:[...new Set([...(state.approvedContacts || []),peer])],
      revokedContacts:(state.revokedContacts || []).filter(id => id !== peer),
      messageRequests:(state.messageRequests || []).filter(id => id !== peer)}));
    scheduleDelivery();
  });
}

export async function confirmAccount() {
  const current = await load();
  if (!current || current.wallet.deviceSecret !== activeDevice || current.join.serverAddr.id !== profile.join.serverAddr.id)
    throw new Error('Your account changed. Reopen Yap.');
  profile = current;
  if (profile.pendingTransfer || profile.pendingRecovery) throw new Error('Finish restoring your account first.');
  const wallet = profile.wallet;
  await rpc({type:'accountRegister',wallet});
  return wallet;
}
function hasLocalActivity(state) {
  return !!Object.keys(state.privateGroupState?.groups || {}).length || !!state.publicProfile || !!Object.keys(state.joinedGroups||{}).length || !!state.nearby || !!state.privacyManaged || !!state.directState?.messages.length || !!state.messageRequests?.length || !!state.username || state.wallet.nextSequence > 1 || state.outbox.length || state.saved.length || state.blocks.length ||
    state.pendingUnblocks.length || state.hideAnonymous || state.recoveryFileCreated || state.recoveryKeyIssued || state.accountRestored;
}
export function canRestoreHere() {
  return profile && !profile.pendingTransfer && !hasLocalActivity(profile);
}
export async function restoreIdentity({account, home, root}) {
  return navigator.locks.request('yap-web-delivery-v1', async () => {
    const original = await load();
    if (original.join.serverAddr.id !== home) throw new Error('This account belongs to a different home.');
    if (original.wallet.chain.at(-1).payload.account === account) return;
    if (original.pendingTransfer || hasLocalActivity(original)) throw new Error('Open a fresh browser profile to restore. This one already has a Yap account in use.');
    const {chain} = await rpc({type:'accountLookup',account});
    if (!chain.length || chain.at(-1).payload.home !== home) throw new Error('Your home could not confirm this account.');
    const pending = original.pendingRecovery;
    const wallet = pending?.chain.at(-1).payload.account === account &&
      (JSON.stringify(pending.chain.slice(0,-1)) === JSON.stringify(chain) ||
       JSON.stringify(pending.chain) === JSON.stringify(chain))
      ? pending : JSON.parse(recover_browser_wallet(root, JSON.stringify(chain), home));
    // Keep the exact device key before registering, so a retry can recover a lost acknowledgment.
    await modify(state => {
      if (state.wallet.deviceSecret !== original.wallet.deviceSecret || hasLocalActivity(state))
        throw new Error('This browser account changed. Open a fresh profile to restore.');
      return {...state,pendingRecovery:wallet};
    });
    const restored = await openClient({...original,wallet});
    try {
      await restored.request(JSON.stringify({type:'accountRegister',wallet}));
      await modify(state => {
        if (state.pendingRecovery?.deviceSecret !== wallet.deviceSecret || hasLocalActivity(state))
          throw new Error('This account changed during recovery. Retry with your file.');
        return {...state,wallet,privateGroupState:null,directState:null,directAvailable:false,messageRequests:[],pendingApprovals:[],approvedContacts:[],revokedContacts:[],inboxPolicy:null,pendingRecovery:null,welcomePending:false,setupStep:'restored-username',username:null,usernameDomain:null,mailboxElsewhere:false,accountRestored:true,safeForPublic:false,posts:[],savedPosts:{},feedCache:{},lastSync:null};
      });
    } finally {await restored.close();}
  });
}

function scheduleDelivery() {
  navigator.serviceWorker
    ?.getRegistration('/app/')
    .then((reg) => reg?.sync?.register('yap-delivery'))
    .catch(() => {});
}

function mergeHistory(previous = [], incoming) {
  const records = new Map(previous.map((record) => [record.event.id, record]));
  for (const record of incoming) {
    if (!records.get(record.event.id)?.deletion) records.set(record.event.id, record);
  }
  return [...records.values()];
}

// Group invitations contain a distinct membership capability. Always send each signed event
// via its own community, including after another tab has switched back to the home feed.
async function scopedRpc(scope, command) {
  if (!scope || scope === profile.join.community) return rpc(command);
  const join = profile.joinedGroups?.[scope]?.join;
  if (!join || join.community !== scope || join.serverAddr.id !== profile.join.serverAddr.id)
    throw new Error('Rejoin this group to continue.');
  const group = await openClient({ ...profile, join });
  try {
    return JSON.parse(await group.request(JSON.stringify(command)));
  } finally {
    await group.close();
  }
}
function accountTask(task) {
  return navigator.locks.request('yap-web-delivery-v1', async () => {
    profile = await load();
    if (
      !profile ||
      activeDevice !== profile.wallet.deviceSecret ||
      profile.pendingRecovery ||
      profile.pendingTransfer
    )
      throw new Error('Finish opening your account first.');
    return task();
  });
}
// The delivery lock also covers service-worker uploads. Remove only the events the
// user reviewed, preserving work queued in another tab and the device sequence.
export function discardUploads(ids) {
  const reviewed = new Set(ids);
  return accountTask(() => modify(state => ({
    ...state, outbox: state.outbox.filter(event => !reviewed.has(event.id))
  })));
}
// The encrypted device store owns ratchets and drafts. Prepare commits the
// advanced ratchet and exact packet under the same lock used by other tabs.
export function privateGroupAction(action) {
  return accountTask(async () => {
    const saved = profile.privateGroupState || null;
    const {state, result} = await rpc({type:'privateGroup', state:saved, action});
    if (JSON.stringify(state) !== JSON.stringify(saved))
      await modify(profile => ({...profile, privateGroupState:state}));
    return result;
  });
}
export function listGroups() {
  return accountTask(async () => {
    const { groups } = await rpc({ type: 'groups' });
    await modify((s) => ({ ...s, groupDirectory: groups }));
  });
}
export function joinGroup(id, details) {
  return accountTask(async () => {
    if (Object.keys(profile.joinedGroups || {}).length >= 32 && !profile.joinedGroups?.[id])
      throw new Error('Your saved group list is full.');
    const result = await rpc(
      details ? { type: 'createGroup', details } : { type: 'joinGroup', group: id },
    );
    if (
      result.group.id !== result.join.community ||
      result.join.serverAddr.id !== profile.join.serverAddr.id
    )
      throw new Error('Invalid group invitation.');
    await modify((s) => ({ ...s, joinedGroups: { ...s.joinedGroups, [result.group.id]: result } }));
    return result.group.id;
  });
}
export function selectGroup(id) {
  return accountTask(async () => {
    if (id && !profile.joinedGroups?.[id]) throw new Error('Join this group first.');
    if ((profile.activeGroup || null) === id) return;
    await modify((s) => changeFeed(s,{activeGroup:id}));
    replicaFeed = undefined;
    changed();
  });
}
export function setNearby(nearby) {
  return accountTask(async () => {
    await modify((s) => changeFeed(s,{nearby}));
    replicaFeed = undefined;
    changed();
  });
}
export async function setPrivacy(change) {
  await modify((s) => {
    const policy = {
      feedDays: 0,
      dmDays: 0,
      approvedOnly: false,
      ...s.inboxPolicy,
      hideAnonymous: s.hideAnonymous,
      ...change,
    };
    return {
      ...s,
      inboxPolicy: policy,
      pendingPrivacy: policy,
      privacyManaged: true,
      hideAnonymous: policy.hideAnonymous,
    };
  });
  scheduleDelivery();
}

function portable(command) {
  return new Promise((resolve, reject) => {
    const worker = new Worker('/app/archive-worker.js', { type: 'module' });
    worker.onmessage = ({ data }) => {
      worker.terminate();
      if (data.error) reject(new Error(data.error));
      else resolve(data.result);
    };
    worker.onerror = () => {
      worker.terminate();
      reject(new Error('Couldn’t open the backup tool on this device.'));
    };
    worker.postMessage(command);
  });
}

export function exportArchive(passphrase, includeHome) {
  return accountTask(async () => {
    if (includeHome) {
      const { events } = await rpc({ type: 'accountExport', wallet: profile.wallet });
      await modify((s) => ({ ...s, ownedEvents: mergeHistory(s.ownedEvents, events) }));
    }
    const s = profile;
    const localData = {
      client: { alias: s.alias, profile: s.publicProfile },
      sourceHomeAddress: s.join.serverAddr,
      preferences: [
        { community: s.join.community, bookmarks: s.saved, hidden: s.hiddenPosts || [] },
      ],
      joinedGroups: [
        ...new Map(
          [
            ...(s.recoveredGroups || []),
            ...Object.values(s.joinedGroups || {}).map((v) => v.group),
          ].map((g) => [g.id, g]),
        ).values(),
      ],
      recoveredPendingEvents: s.recoveredDrafts || [],
      browser: { nearby: s.nearby, muted: s.muted },
      historyCoverage: includeHome ? 'local-plus-home' : 'local-receipts',
    };
    const { ciphertext } = await portable({
      type: 'encrypt',
      wallet: { ...s.wallet, pending: s.outbox },
      events: s.ownedEvents || [],
      localData,
      passphrase,
    });
    const bytes = new Uint8Array(ciphertext.length / 2);
    for (let i = 0; i < bytes.length; i++)
      bytes[i] = parseInt(ciphertext.slice(i * 2, i * 2 + 2), 16);
    return new Blob([bytes], { type: 'application/octet-stream' });
  });
}
const head = (chain) => chain.at(-1).payload;
function compatible(left, right) {
  return left
    .slice(0, Math.min(left.length, right.length))
    .every((r, i) => r.signature === right[i].signature);
}
export async function importArchive(file, passphrase, invitation, allowDrafts) {
  if (file.size > 16 * 1024 * 1024 + 65536) throw new Error('This backup is too large.');
  const bytes = new Uint8Array(await file.arrayBuffer());
  let ciphertext = '';
  for (let i = 0; i < bytes.length; i += 8192)
    ciphertext += Array.from(bytes.subarray(i, i + 8192), (v) =>
      v.toString(16).padStart(2, '0'),
    ).join('');
  const { bundle } = await portable({ type: 'decrypt', ciphertext, passphrase });
  const old = bundle.wallet,
    archive = bundle.archive.payload,
    meta = archive.localData || {};
  if (old.pending.length && !allowDrafts)
    throw new Error('Choose to keep unsent items as drafts before importing.');
  if (!profile) await start('restore');
  return navigator.locks.request('yap-web-delivery-v1', async () => {
    const current = await load(),
      existing = head(current.wallet.chain),
      account = archive.account;
    if (
      activeDevice !== current.wallet.deviceSecret ||
      current.pendingRecovery ||
      current.outbox.length ||
      current.directState?.outbox.length
    )
      throw new Error('Finish restoring or sending queued items first.');
    if (existing.account !== account && hasLocalActivity(current))
      throw new Error(
        'This browser holds a different account. Open a fresh browser profile to restore.',
      );
    let join = current.join;
    if (invitation.trim()) {
      try {
        join = JSON.parse(invitation);
      } catch {
        throw new Error('Check the home invitation and try again.');
      }
    }
    const dest = join?.serverAddr?.id;
    if (!dest || join.community !== dest + '/public')
      throw new Error('Choose a public home invitation.');
    const sameHome = dest === current.join.serverAddr.id;
    const route = {
      join,
      fullHomeStandby: sameHome ? current.fullHomeStandby : null,
      replica: sameHome ? current.replica : null,
    };
    const pending = current.pendingTransfer;
    if (pending && (pending.signature !== bundle.archive.signature || pending.destination !== dest))
      throw new Error('Use the same backup and home to finish this restore.');
    const probe = await BrowserClient.open(
      JSON.stringify({
        join,
        authorSecret: old.deviceSecret,
        nextSequence: old.nextSequence,
        fullHomeStandby: route.fullHomeStandby,
      }),
    );
    let known;
    try {
      known = JSON.parse(
        await probe.request(JSON.stringify({ type: 'accountLookup', account })),
      ).chain;
    } finally {
      await probe.close();
    }
    const chains = [
      old.chain,
      known,
      ...(existing.account === account ? [current.wallet.chain] : []),
      ...(pending ? [pending.wallet.chain] : []),
    ];
    if (!chains.every((a) => chains.every((b) => compatible(a, b))))
      throw new Error(
        'This home has conflicting account history. Your browser account is unchanged.',
      );
    const latest = chains.reduce((a, b) => (a.length >= b.length ? a : b));
    if (pending && latest.length > pending.wallet.chain.length)
      throw new Error('This account has since changed on another device.');
    const wallet =
      pending?.wallet ||
      (await portable({ type: 'recover', rootSecret: old.rootSecret, chain: latest, home: dest }))
        .wallet;
    await modify((s) => ({
      ...s,
      pendingTransfer: { wallet, destination: dest, signature: bundle.archive.signature },
    }));
    const restored = await openClient({ ...route, wallet });
    try {
      const result = JSON.parse(
        await restored.request(
          JSON.stringify({ type: 'accountImport', wallet, events: archive.events }),
        ),
      );
      const drafts = [
        ...(current.recoveredDrafts || []),
        ...(meta.recoveredPendingEvents || []),
        ...old.pending,
      ];
      const prefs = (meta.preferences || []).filter((p) => p.community === join.community);
      const sameAccount = existing.account === account && sameHome;
      const saved = await load();
      const retained = mergeHistory(sameAccount ? saved.ownedEvents : [], result.events);
      await modify((s) =>
        purgeDeleted(
          {
            ...s,
            ...route,
            wallet,
            pendingTransfer: null,
            welcomePending: false,
            privateGroupState: null,
            setupStep: 'restored-username',
            mailboxElsewhere: false,
            accountRestored: true,
            alias: meta.client?.alias || s.alias,
            publicProfile: meta.client?.profile,
            username: sameAccount ? s.username : null,
            usernameDomain: sameHome ? s.usernameDomain : null,
            posts: [],
            savedPosts: {},
            feedCache: {},
            groupLocality: null,
            outbox: [],
            activeGroup: null,
            joinedGroups: sameAccount ? s.joinedGroups : {},
            groupDirectory: [],
            lastSync: null,
            blocks: sameAccount ? s.blocks : [],
            pendingUnblocks: sameAccount ? s.pendingUnblocks : [],
            safeForPublic: false,
            directState: null,
            directAvailable: false,
            pendingApprovals: [],
            approvedContacts: [],
            revokedContacts: [],
            messageRequests: [],
            saved: [
              ...new Set([
                ...(sameAccount ? s.saved : []),
                ...prefs.flatMap((p) => p.bookmarks || []),
              ]),
            ],
            hiddenPosts: [
              ...new Set([
                ...(sameAccount ? s.hiddenPosts || [] : []),
                ...prefs.flatMap((p) => p.hidden || []),
              ]),
            ],
            nearby: meta.browser?.nearby || null,
            muted: meta.browser?.muted || {},
            recoveredGroups: Array.isArray(meta.joinedGroups) ? meta.joinedGroups : [],
            ownedEvents: retained,
            recoveredDrafts: [...new Map(drafts.map((d) => [d.id, d])).values()],
            moveNotice:
              meta.sourceHomeAddress?.id === head(old.chain).home &&
              meta.sourceHomeAddress.id !== dest
                ? { address: meta.sourceHomeAddress, chain: wallet.chain }
                : null,
          },
          retained.filter((r) => r.deletion).map((r) => r.event.id),
        ),
      );
    } finally {
      await restored.close();
    }
    location.reload();
  });
}
export function resetLocalData() {
  return navigator.locks.request('yap-web-delivery-v1', async () => {
    await clearDevice((s) => {
      if (!s || s.wallet.deviceSecret !== activeDevice)
        throw new Error('Reopen the active account first.');
    });
    channel.postMessage('changed');
    location.reload();
  });
}
