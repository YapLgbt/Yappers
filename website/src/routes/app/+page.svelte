<script lang="ts">
  import ActionButton from '../../lib/ActionButton.svelte';
  import { onMount, tick } from 'svelte';
  import { MediaQuery } from 'svelte/reactivity';
  import {pushState, replaceState} from '$app/navigation';
  import {page} from '$app/state';
  import NeighborhoodNav from '../../lib/NeighborhoodNav.svelte';
  import ContentMenu from '../../lib/ContentMenu.svelte';
  import AppIcon from '../../lib/AppIcon.svelte';
  import AppHead from '../../lib/AppHead.svelte';
  import Report from '../../lib/Report.svelte';
  import Inbox from '../../lib/Inbox.svelte';
  import FeedPost from '../../lib/FeedPost.svelte';
  import Welcome, { type WelcomeStep } from '../../lib/Welcome.svelte';
  import Composer from '../../lib/Composer.svelte';
  import Preferences from '../../lib/Preferences.svelte';
  import Username from '../../lib/Username.svelte';
  import Recovery from '../../lib/Recovery.svelte';
  import Filters from '../../lib/Filters.svelte';
  import Groups from '../../lib/Groups.svelte';
  import Nearby from '../../lib/Nearby.svelte';
  import ProfileEdit from '../../lib/ProfileEdit.svelte';
  import AccountData from '../../lib/AccountData.svelte';
  import ProfileSummary from '../../lib/ProfileSummary.svelte';
  import Block from '../../lib/Block.svelte';
  import { pageMotion } from '../../lib/pageMotion';
  import { deliveryDisclosure } from '../../lib/deliveryDisclosure';
  import { pullToRefresh } from '../../lib/pull';
  import { trackFeed } from '../../lib/feedMotion.svelte';
  import { loadCommunity, visible, authorLabel, errorMessage, type Community, type Profile,
    type Author, type Contact, type BlockedAccount, type Post, type RecoveryMode, type View } from '../../lib/community';

  let client = $state.raw<Community | null>(null), profile = $state.raw<Profile | null>(null);
  let replica = $state.raw<Community['replicaFeed']>();
  let step = $state<WelcomeStep>(null), view = $state<View>('connecting'), ready = $state(false);
  let contact = $state<Contact | null>(null), inboxError = $state('');
  let destination = $state<'feed' | 'groups' | 'saved' | 'you'>('feed');
  let mode = $state<'feed' | 'groups' | 'saved' | 'inbox' | 'you'>('feed'), sort = $state('new'), topic = $state('All');
  let notice = $state(''), connection = $state(''), busy = $state(false);
  let composer = $state<Composer>();
  let reportDialog = $state<Report>();
  let preferences = $state<Preferences>();
  let username = $state<Username>();
  let filters = $state<Filters>();
  let block = $state<Block>();
  let recovery = $state<Recovery>();

  // The history entry contains only view names and a tab-local reference. Private contacts stay out of browser history.
  const historyContacts=new Map<string,Contact>();
  type Screen = { mode: typeof mode; destination: typeof destination; conversation?: string; group?: string; inboxParent?: 'screen'|'folder';
    thread?: string; comments?: boolean; returnY?: number; returnFocus?: string; returnIndex?: number; anchorTop?: number };
  const historyThreads = new Map<string,string>();
  const historyGroups = new Map<string,string>();
  let groupChanges = $state(0);

  function snapshot(): Screen {
    const id = client?.profile?.activeGroup;
    if (!id) return {mode, destination};
    const known = [...historyGroups].find(([, value]) => value === id);
    const group = known?.[0] ?? crypto.randomUUID();
    historyGroups.set(group, id);
    return {mode, destination, group};
  }
  async function selectScope(id: string | null) {
    if (!client) return;
    groupChanges++;
    try { await client.selectGroup(id); }
    finally { groupChanges--; }
  }
  const wideReading = new MediaQuery('(min-width: 76rem)');
  let thread = $state<string | undefined>();
  const screens = ['feed', 'groups', 'groupFeed', 'saved', 'you', 'inbox', 'conversation'];
  const screen = $derived(mode === 'inbox' && contact ? 'conversation'
    : mode === 'feed' && destination === 'groups' ? 'groupFeed' : mode);
  // A thread covers its source only on layouts that show one pane.
  const readingPosition = $derived(screens.indexOf(screen) * 2 + Number(!!thread && !wideReading.current));
  let lastHistory:object|undefined;
  function openThread(post: Post, comments: boolean) {
    const token=crypto.randomUUID();
    historyThreads.set(token,post.id);
    if (thread) {
      replaceState('', {yap: {...snapshot(),thread:token,comments}});
      return;
    }
    const trigger = document.getElementById((comments ? 'comments-' : 'read-') + post.id);
    const row = trigger?.closest('article');
    const returnIndex = row ? Array.from(document.querySelectorAll('#posts > article')).indexOf(row) : 0;
    const entry = snapshot();
    const anchorTop = trigger?.getBoundingClientRect().top;
    replaceState('', {yap: {...entry, returnY:window.scrollY, returnFocus:token, returnIndex, anchorTop, comments}});
    pushState('', {yap: {...entry, thread:token, comments, anchorTop}});
  }
  $effect(() => {
    const state = page.state as {yap?:Screen};
    if (!ready || state === lastHistory) return;
    const first = lastHistory === undefined;
    lastHistory = state;
    if (first && !state.yap) {
      if (profile?.activeGroup) destination = 'groups';
      replaceState('', {yap:snapshot()});
      return;
    }
    void restoreScreen(state, first);
  });
  async function restoreScreen(state: {yap?:Screen}, first: boolean) {
    const saved = state.yap;
    const group = saved?.group ? historyGroups.get(saved.group) : null;
    // A tab-local reference may expire on reload. Show the directory if its group is unknown.
    if (saved?.group && !group && saved.destination === 'groups') {
      replaceState('', {yap: {mode:'groups', destination:'groups'}});
      return;
    }
    if (client && (groupChanges || (client.profile?.activeGroup || null) !== (group || null))) {
      try { await selectScope(group || null); }
      catch (error) {
        if (page.state === state) {
          notice = errorMessage(error);
          mode = destination = 'groups';
          thread = undefined; contact = null;
          replaceState('', {yap:snapshot()});
        }
        return;
      }
      if (page.state !== state) return;
      void refresh();
    }
    mode = saved?.mode || 'feed';
    destination = saved?.destination || 'feed';
    thread = saved?.thread ? historyThreads.get(saved.thread) : undefined;
    contact = saved?.conversation ? historyContacts.get(saved.conversation) || null : null;
    await tick();
    if (page.state !== state) return;
    if (thread && saved?.thread) {
      const heading = document.getElementById(saved.comments ? 'thread-comments' : 'thread-heading')
        || document.getElementById('thread-heading');
      heading?.focus({preventScroll:true});
      if (wideReading.current) {
        const trigger = document.getElementById((saved.comments ? 'comments-' : 'read-') + thread);
        if (trigger && saved.anchorTop !== undefined)
          window.scrollBy(0, trigger.getBoundingClientRect().top - saved.anchorTop);
        const pane = document.getElementById('thread-pane');
        if (pane) pane.scrollTop = saved.comments && heading
          ? heading.getBoundingClientRect().top - pane.getBoundingClientRect().top + pane.scrollTop - 16 : 0;
      } else if (saved.comments && heading) heading.scrollIntoView({block:'start'});
      else window.scrollTo(0,0);
      return;
    }
    if (saved?.returnFocus) {
      const id = historyThreads.get(saved.returnFocus);
      const rows = document.querySelectorAll('#posts > article');
      const trigger = document.getElementById((saved.comments ? 'comments-' : 'read-') + id)
        || rows[Math.min(saved.returnIndex ?? 0, rows.length - 1)]?.querySelector<HTMLElement>('.read-post');
      if (trigger && saved.anchorTop !== undefined) {
        trigger.focus({preventScroll:true});
        window.scrollBy(0, trigger.getBoundingClientRect().top - saved.anchorTop);
        return;
      }
    }
    if (!first) {
      await focusPageHeading();
      window.scrollTo(0, saved?.returnY ?? 0);
    }
  }
  function remember(person:Contact|null=null, inboxParent?: Screen['inboxParent']) {
    const previous = (page.state as {yap?:Screen}).yap;
    if (previous && !previous.thread)
      replaceState('', {yap:{...previous, returnY:window.scrollY, returnFocus:undefined}});
    const conversation = person ? crypto.randomUUID() : undefined;
    if (person && conversation) historyContacts.set(conversation,person);
    return pushState('', {yap:{...snapshot(),conversation,inboxParent}});
  }
  async function focusPageHeading(){
    await tick();
    const heading=document.querySelector<HTMLElement>(thread ? '#thread-heading' : '#neighborhood .reading-list h1');
    if(heading){heading.tabIndex=-1;heading.focus({preventScroll:true});}
  }
  let pendingSync: Promise<void> | null = null;

  let posts = $derived.by(() => {
    if (!profile || !client) return [];
    const feed = view === 'replica' && replica && client.canShowPublic(profile) ? replica.posts : profile.posts;
    const current = profile;
    const scope=current.activeGroup||current.join.community;
    const fresh=[...client.queuedPosts(current, feed), ...feed.map(p=>({...p,scope}))];
    const source=mode==='saved'?[...new Map([...Object.values(current.savedPosts||{}),...fresh].map(p=>[p.id,p])).values()]:fresh;
    const result = source.filter(post => visible(post,current) && !current.muted?.[post.id] && !current.hiddenPosts?.includes(post.id) &&
      (mode === 'you' ? post.mine : mode === 'saved' ? current.saved.includes(post.id) : topic === 'All' || post.topic === topic));
    const votes = new Map(current.outbox.filter(e=>e.payload.action.kind==='vote').map(e=>[e.payload.action.target,e.payload.action.value]));
    const adjusted=result.map(post=>{
      const value=votes.get(post.id);
      return value===undefined ? post : {...post,vote:value,score:(post.score||0)-(post.vote||0)+value};
    });
    return mode === 'feed' && sort === 'popular' ? adjusted.sort((a,b) => (b.score || 0) - (a.score || 0)) : adjusted;
  });
  let feedList = $state<HTMLDivElement>();
  const feedScope = $derived(ready && !step && !thread && profile && view !== 'connecting' && (view !== 'offline' || profile.lastSync) &&
    (mode === 'feed' || mode === 'saved' || mode === 'you')
    ? JSON.stringify([mode, profile.activeGroup, profile.nearby, topic]) : null);
  trackFeed(() => feedList, () => feedScope, () => posts);
  let scopedOutbox = $derived(profile?.outbox.filter(event =>
    event.payload.community === (profile?.activeGroup || profile?.join.community)) || []);
  let hasQueuedActions = $derived(scopedOutbox.length > 0);
  let showQueuedActions = $state(false);
  $effect(() => {
    if (!hasQueuedActions) { showQueuedActions = false; return; }
    // Routine acknowledgements finish before a global delivery indicator is needed.
    const timeout = setTimeout(() => { showQueuedActions = true; }, 400);
    return () => clearTimeout(timeout);
  });
  let deliveryLabel = $derived([
    view === 'offline' ? (connection || 'Home unavailable') : view === 'replica' ? 'Cached feed' : '',
    scopedOutbox.length && (showQueuedActions || view === 'offline' || view === 'replica') ? scopedOutbox.length + ' waiting to sync' : ''
  ].filter(Boolean).join(' · ') || notice);
  let filterCount = $derived(Number(sort !== 'new') + Number(topic !== 'All'));
  let threadPost = $derived(thread ? posts.find(post => post.id === thread) : undefined);
  let waiting = $derived(!profile?.lastSync && view !== 'replica');
  let nextRadius = $derived(Math.min(250, (profile?.nearby?.radiusMiles || 25) * 2));
  let widening = $state(false);
  async function widenArea() {
    const nearby = profile?.nearby;
    if (!client || !nearby || widening) return;
    widening = true;
    try {
      await client.setNearby(nearby.radiusMiles < 250 ? { ...nearby, radiusMiles: nextRadius } : null);
      await refresh();
      if (mode === 'feed' && !thread) await focusPageHeading();
    } finally {
      widening = false;
    }
  }
  function readProfile() {
    profile = client?.profile ?? null;
    replica = client?.replicaFeed;
    inboxError = profile?.mailboxElsewhere ? client!.otherMessagingDevice : client?.inboxError || '';
  }
  async function refresh() {
    if (!client || !ready) return;
    if (pendingSync) return pendingSync;
    const home = client;
    busy = true;
    pendingSync = (async () => {
      try {
        view = await home.sync(); if(mode==='groups') await home.listGroups().catch(()=>{}); connection = '';
        notice = view === 'replica' && home.replicaFeed
          ? 'Showing posts from '+new Date(home.replicaFeed.asOf * 1000).toLocaleString()+'. New posts will send when you reconnect.' : '';
      } catch {
        view = 'offline'; connection = navigator.onLine ? '' : 'Offline';
        notice = profile?.lastSync ? 'Showing saved posts. New posts will send when you reconnect.' : 'Couldn’t load posts. Try refreshing.';
      } finally { readProfile(); busy = false; pendingSync = null; }
    })();
    return pendingSync;
  }
  const pull = pullToRefresh(() => ready && !busy, refresh);
  async function deliver() {
    if (pendingSync) await pendingSync;
    if (client?.profile?.outbox.length) await refresh();
  }
  async function editState(change: (p: Profile) => Profile) {
    if (!client) return;
    await client.modify(change);
  }
  async function act(task: () => Promise<void>) {
    try { await task(); } catch (problem) { notice = errorMessage(problem); }
  }
  function openRecovery(mode: RecoveryMode) { void act(() => recovery!.open(mode)); }
  function save(post: Post) {
    return act(() => editState(s => {
      const id=post.id,savedPosts={...s.savedPosts};
      if(s.saved.includes(id)){delete savedPosts[id];return {...s,saved:s.saved.filter(key=>key!==id),savedPosts}}
      return {...s,saved:[...s.saved,id],savedPosts:{...savedPosts,[id]:post}};
    }));
  }
  function hide(id: string) {
    void act(async () => {
      await editState(s => ({ ...s, hiddenPosts: [...new Set([...(s.hiddenPosts || []),id])] }));
      notice = 'Post hidden on this device.';
    });
  }
  function remove(id: string, label: string, scope?:string) {
    if (!window.confirm('Delete this '+label+' from your home and send the deletion to other homes?')) return;
    void act(async () => { await client!.queue({kind:'delete',target:id},null,scope); await deliver(); });
  }
  function blockAccount(author: Author) {
    void act(async () => {
      await client!.blockAccount({event:author.id,peer:author.visibility?.peer,label:authorLabel(author),scope:author.scope});
      await refresh();
    });
  }
  async function blockContact(person: Contact) {
    await client!.blockAccount({event:'dm-'+person.peer,byAccount:true,peer:person.peer,label:person.label});
    await refresh();
  }
  async function message(author: Author) {
    const origin = page.state;
    try {
      const recipient = await client!.messageContact(author.id,author.scope);
      if (page.state !== origin) return;
      await openInbox();
      contact = recipient;
      remember(contact, 'folder');
      notice = '';
    } catch { notice = client?.profile?.mailboxElsewhere ? client.otherMessagingDevice : 'Couldn’t open this conversation. Refresh and try again.'; }
  }
  function navigate(next: typeof destination,area:typeof destination=next) {
    if (next === 'feed' && area === 'feed' && client?.profile?.activeGroup) {
      void act(() => openGroup(null));
      return;
    }
    const changed = destination !== area || mode !== next || !!contact || !!thread;
    destination=area; mode=next; contact=null; thread=undefined;
    if (changed) remember();
    else { window.scrollTo(0,0); void focusPageHeading(); }
  }
  async function openGroup(id:string|null) {
    if (!client) return;
    const origin = page.state;
    await selectScope(id);
    if (page.state !== origin) return;
    sort='new';topic='All';navigate('feed',id?'groups':'feed');await refresh();
  }
  async function vote(post:Post,direction:number) {
    await act(async()=>{await client!.queue({kind:'vote',target:post.id,value:post.vote===direction?0:direction},null,post.scope);await deliver()});
  }
  function mute(post:Post) {void act(()=>editState(s=>({...s,muted:{...s.muted,[post.id]:post.headline||post.body.slice(0,60)}})))}
  async function openInbox() {
    contact = null; mode = 'inbox';
    await remember(null, 'screen');
    void refresh();
  }
  function changeContact(person: Contact | null) {
    if (person) remember(person, 'folder');
    else if ((page.state as {yap?:Screen}).yap?.inboxParent === 'folder') history.back();
    else replaceState('', {yap:snapshot()});
  }
  function closeInbox() {
    if ((page.state as {yap?:Screen}).yap?.inboxParent === 'screen') history.back();
    else navigate(destination);
  }
  function unblock(account: BlockedAccount) {
    void act(async () => {
      await client!.unblockAccount(account);
      await refresh();
    });
  }
  async function enter() {
    if (client?.profile?.setupStep) await client.verifyWelcomeUsername(true);
    step = null; ready = true; await refresh();
  }
  async function boot() {
    try {
      if (!client) client = await loadCommunity();
      client.updates.addEventListener('change', readProfile);
      if (!await client.start()) { step = 'intro'; return; }
      readProfile();
      if (profile?.setupStep) { step = 'username'; return; }
      step = null; ready = true; await refresh();
    } catch {
      connection = 'Connection paused'; view = 'offline';
      notice = 'Couldn’t open Yap. Check your connection and allow browser storage, then refresh.';
    }
  }
  function refreshOrBoot() { return ready ? refresh() : boot(); }
  onMount(() => {
    void boot();
    if ('serviceWorker' in navigator && !import.meta.env.DEV) navigator.serviceWorker.register('/app/background-sw.js',{scope:'/app/',type:'module'})
      .catch(()=>navigator.serviceWorker.register('/app/sw.js',{scope:'/app/'}))
      .then(async registration=>{
        const periodic=(registration as ServiceWorkerRegistration & {periodicSync?:{register:(tag:string,options:{minInterval:number})=>Promise<void>}}).periodicSync;
        if(periodic && (await navigator.permissions.query({name:'periodic-background-sync' as PermissionName})).state==='granted')
          await periodic.register('yap-refresh',{minInterval:24*60*60*1000});
      }).catch(()=>{});
    const flag = '/app/flag-print.js';
    void import(/* @vite-ignore */ flag).catch(() => {});
    const listeners = new AbortController();
    const opts = { signal: listeners.signal };
    window.addEventListener('online', () => { if (ready) void refresh(); },opts);
    window.addEventListener('offline', () => { view = 'offline'; connection = 'Offline'; },opts);
    document.addEventListener('visibilitychange', () => { if (!document.hidden && ready) void refresh(); },opts);
    const timer = setInterval(() => {
      if (ready && navigator.onLine && !document.hidden && !document.querySelector('dialog[open], [role=dialog][data-state=open]')) void refresh();
    },60000);
    const inboxTimer = setInterval(() => { if (ready && mode === 'inbox' && !document.hidden && navigator.onLine && !busy) void refresh(); },10000);
    return () => { listeners.abort(); clearInterval(timer); clearInterval(inboxTimer); client?.updates.removeEventListener('change',readProfile); };
  });
</script>
<AppHead />
<header class="app-header" hidden={step === "intro"} class:enrolled={ready && step === null}><div class="bar">
  <div class="bar-start" hidden={step !== null}>
    {#if mode !== 'inbox'}<ActionButton id="inbox-tab" disabled={!ready} onclick={openInbox}>Inbox</ActionButton>{/if}
    <button id="refresh" class="icon-action mtrl-icon-button mtrl-icon-button--standard" aria-label="Refresh" disabled={busy} onclick={refreshOrBoot}><AppIcon name="refresh" /></button>
  </div>
  <a href="/" class="brand" aria-label="Yap home"><img src="/assets/yap-wordmark.svg" width="72" height="36" alt="yap"></a>
  <div class="bar-end" hidden={step !== null}>
    <span id="connection" class="sr-only" role="status">{connection}</span>
    {#if mode !== 'inbox' && mode !== 'groups'}<ActionButton id="compose" variant="filled" disabled={!ready || groupChanges > 0} onclick={event => composer?.open(null, event.currentTarget)}><AppIcon name="post" />Post</ActionButton>{/if}
  </div>
</div>
{#if ready && step === null}
  <NeighborhoodNav current={mode === 'inbox' ? null : destination}
    select={id => navigate(id)} />
{/if}
</header>
<Welcome {client} bind:step create={async (name,invitation) => { if (!client || !await client.start('create',name,invitation)) throw new Error('Couldn’t join your home. Try again.'); readProfile(); }} {enter} recovery={openRecovery} />
{#snippet feedPost(post: Post, current: Profile, detail: boolean)}
  <FeedPost {detail} {openThread} report={(kind,id,text)=>reportDialog?.open(kind,id,text)} {vote} {mute} {message}
    {post} profile={current} replica={view === 'replica'} reply={(item, trigger) => composer?.open(item, trigger)}
    {save} {hide} block={author => block?.open(author)} {remove} />
{/snippet}
<main id="neighborhood" class:split-thread={!!thread} hidden={step !== null} use:pull>
  <div class="reading-list" use:pageMotion={ready && step === null ? readingPosition : null}>
  {#if mode === 'you' && profile}
    <ProfileSummary name={profile.alias} username={profile.username} details={profile.publicProfile} own>
      {#if !profile.username}<ActionButton onclick={() => username?.open()}>Choose a username</ActionButton>{/if}
    </ProfileSummary>
    <div class="profile-actions">{#if client}<ProfileEdit {client} {profile}/>{/if}<ActionButton id="settings" variant="tonal" onclick={(event) => preferences?.open(false, event.currentTarget)}>Preferences</ActionButton>
      {#if client}<AccountData {client} {profile}/>{/if}</div>
    <h2 class="section-title">Your posts</h2>
  {:else if mode !== 'groups' && (mode !== 'inbox' || !contact)}
    <section class="feed-heading" class:nearby-heading={mode === 'feed'}>
      <div class="heading">
        <h1 id="feed-heading">{mode === 'inbox' ? 'Inbox' : mode === 'saved' ? 'Saved' : profile?.joinedGroups?.[profile.activeGroup || '']?.group.name || profile?.nearby?.center.label || 'All cities'}</h1>
        {#if mode === 'feed'}
          <ActionButton id="feed-filter" variant={filterCount ? 'tonal' : 'outlined'} aria-haspopup="dialog" onclick={(event) => filters?.open(sort, topic, event.currentTarget)}><AppIcon name="filter" />Filter{filterCount ? ' (' + filterCount + ')' : ''}</ActionButton>
        {:else if mode === 'inbox'}
          <ContentMenu label="Inbox settings" items={[
            {label:'Privacy',run:()=>preferences?.open()}, {label:'Blocked accounts',run:()=>preferences?.open(true)}
          ]}/>
          <ActionButton onclick={closeInbox}>Done</ActionButton>
        {/if}
      </div>
      {#if mode === 'feed' && profile && client}
        <div class="feed-home">
          {#if profile.activeGroup}
            <ActionButton onclick={() => act(() => openGroup(null))}>Back to nearby</ActionButton>
          {:else}
            {#if profile.nearby}<span>Within {profile.nearby.radiusMiles} {profile.nearby.radiusMiles === 1 ? 'mile' : 'miles'}</span>{/if}
            <Nearby {profile} {client} {refresh}/>
          {/if}
          {#if topic !== 'All'}<span>Topic <strong>{topic}</strong></span>{/if}
        </div>
      {/if}
    </section>
  {/if}
  {#if mode === 'groups' && profile && client}
    <Groups {profile} {client} {openGroup}/>
  {:else if mode === 'inbox' && profile && client}
    <Inbox changedContact={changeContact} report={(kind,id,text)=>reportDialog?.open(kind,id,text)} {client} {profile} bind:contact {refresh} browse={() => navigate('feed')} block={blockContact}
      status={profile.mailboxElsewhere ? client.otherMessagingDevice : view === 'offline' || view === 'replica' ? 'Offline inbox. Queued messages will send when you reconnect.' :
        inboxError || (view === 'primary' && !profile.directAvailable ? 'Messaging is unavailable on this home.' : '')} />
  {:else}
  <div id="posts" bind:this={feedList} aria-busy={view === 'connecting' || (mode === 'feed' && waiting && busy)} data-feed-state={view}>
    {#if profile}
      {#each posts as post (post.id)}
        {@render feedPost(post, profile, false)}
      {:else}
        <section class="empty">
          {#if mode === 'feed' && (view === 'connecting' || (busy && waiting))}
            <h2>Loading posts…</h2>
          {:else if mode === 'feed' && (view === 'offline' || view === 'replica')}
            <h2>No cached posts in this view</h2>
            <p>Reconnect to check for new posts.</p>
            {#if topic !== 'All'}<ActionButton variant="tonal" onclick={() => topic = 'All'}>Show all topics</ActionButton>
            {:else}<ActionButton variant="tonal" onclick={event => composer?.open(null, event.currentTarget)}>Write a post</ActionButton>{/if}
          {:else if mode === 'feed' && topic !== 'All'}
            <h2>No posts in {topic}</h2><ActionButton variant="tonal" onclick={() => topic = 'All'}>Show all topics</ActionButton>
          {:else if mode === 'feed' && profile.nearby && !profile.activeGroup && view === 'primary' && !waiting}
            <h2>No posts to show within {profile.nearby.radiusMiles} {profile.nearby.radiusMiles === 1 ? 'mile' : 'miles'}</h2>
            <p>Write a post for {profile.nearby.center.label} or look a little farther away.</p>
            <div class="quiet-area-actions">
              <ActionButton variant="filled" onclick={event => composer?.open(null, event.currentTarget)}>Write a post</ActionButton>
              <ActionButton variant="outlined" disabled={widening} onclick={() => act(widenArea)}>
                {profile.nearby.radiusMiles < 250 ? 'Widen to ' + nextRadius + ' miles' : 'Switch to all cities'}
              </ActionButton>
            </div>
          {:else}
            <span class="empty-icon"><AppIcon name={mode === 'saved' ? 'saved' : 'nearby'} /></span>
            <h2>{mode === 'saved' ? 'No saved posts yet' : mode === 'you' ? 'No posts yet' : waiting ? 'Waiting for the neighborhood' : 'No posts yet'}</h2>
            <p>{mode === 'saved' ? 'Tap the bookmark on a post to save it here.' : mode === 'you' ? 'Your posts will appear here.' : waiting ? 'Connect to your home to load the feed.' : profile.hideAnonymous || profile.blocks.length ? 'No posts match your current preferences.' : 'Start a conversation in your neighborhood.'}</p>
            {#if mode === 'saved'}<ActionButton variant="tonal" onclick={() => navigate('feed')}>Explore nearby</ActionButton>
            {:else if !waiting}<ActionButton variant="tonal" onclick={event => composer?.open(null, event.currentTarget)}>Write a post</ActionButton>{/if}
          {/if}
        </section>
      {/each}
    {:else}<p class="wait">{view === 'offline' ? 'Couldn’t open your feed.' : 'Opening your feed…'}</p>{/if}
  </div>
  {/if}
  </div>
  {#if thread}
    <section id="thread-pane" class="thread-pane" aria-labelledby="thread-heading">
      <div class="heading thread-heading">
        <h1 id="thread-heading" tabindex="-1">Conversation</h1>
        <ActionButton onclick={()=>history.back()}>Back to {mode === 'you' ? 'profile' : mode === 'saved' ? 'saved posts' : 'feed'}</ActionButton>
      </div>
      {#if threadPost && profile}
        {#key thread}
          {@render feedPost(threadPost, profile, true)}
        {/key}
      {:else}
        <section class="empty"><h2>Conversation unavailable</h2><p>This post is no longer in this view.</p></section>
      {/if}
    </section>
  {/if}
  {#if mode !== 'inbox' && mode !== 'groups'}
    <p id="notice" class="sr-only" role="status">{[deliveryLabel, notice !== deliveryLabel ? notice : ''].filter(Boolean).join('. ')}</p>
    {#if deliveryLabel}
      <details class="delivery-feedback" use:deliveryDisclosure={focusPageHeading}>
        <summary>{deliveryLabel}</summary>
        <div>
          {#if notice && notice !== deliveryLabel}<p>{notice}</p>
          {:else if view === 'offline'}<p>New posts will send when you reconnect.</p>{/if}
          {#if scopedOutbox.length}
            <ul>
              {#each scopedOutbox as event (event.id)}
                {@const a = event.payload.action}
                <li>{a.kind === 'vote' ? 'Vote waiting to sync' : a.kind === 'delete' ? 'Deletion waiting to sync' :
                  (a.kind === 'reply' ? 'Reply' : 'Post') + ' waiting to send · ' + (a.headline || a.body)}</li>
              {/each}
            </ul>
          {/if}
          <ActionButton onclick={refreshOrBoot} disabled={busy}>Refresh</ActionButton>
        </div>
      </details>
    {/if}
  {/if}
  <noscript>Turn on JavaScript to connect to the neighborhood.</noscript>
</main>
<Filters bind:this={filters} apply={(s,t) => { sort = s; topic = t; }} />
<Recovery bind:this={recovery} />
{#if client && profile}
  <Composer bind:this={composer} {profile} domain={client.usernameDomain(profile)}
    queue={(action, scope) => client!.queue(action, null, scope)}
    showReply={async (parent, id) => {
      await tick();
      if (thread === parent) {
        const reply = document.getElementById('reply-' + id);
        reply?.focus({ preventScroll: true });
        reply?.scrollIntoView({ block: 'nearest' });
      }
    }} chooseUsername={() => username?.open()} {deliver} />
  <Report bind:this={reportDialog} {client} />
  <Preferences discard={ids=>client!.discardUploads(ids)} reports={lookup=>reportDialog?.open("account","","",lookup)} bind:this={preferences} {profile} domain={client.usernameDomain(profile)} {pull}
    chooseUsername={() => username?.open()} recovery={openRecovery} {refresh} guide={() => step = 'returning'}
    setPrivacy={change=>act(async()=>{await client!.setPrivacy(change);await refresh()})} unmute={id=>act(()=>editState(s=>{const muted={...s.muted};delete muted[id];return {...s,muted}}))} {unblock} />
  <Username bind:this={username} {client} domain={client.usernameDomain(profile)} />
  <Block bind:this={block} confirm={blockAccount} />
{/if}
