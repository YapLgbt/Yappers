<script lang="ts">
  import { deliveryDisclosure } from './deliveryDisclosure';
  import { pageMotion } from './pageMotion';
  import ActionButton from './ActionButton.svelte';
  import AppIcon from './AppIcon.svelte';
  import Avatar from './Avatar.svelte';
  import ContentMenu from './ContentMenu.svelte';
  import { onDestroy, tick } from 'svelte';
  import { Tabs } from '@skeletonlabs/skeleton-svelte';
  import { inboxFor, inboxPhotos, type ActivityItem, type Community, type Profile, type Contact } from './community';

  let { tab=$bindable('chats'),openActivity, profile, client, contact = $bindable(null), status, refresh, browse, block, report, changedContact }: {
    tab?:string;openActivity:(item:ActivityItem)=>Promise<void>;
    changedContact:(person:Contact|null)=>void;report: (kind:"message"|"account",id:string,text:string)=>void; profile: Profile; client: Community; contact: Contact | null; status: string;
    refresh: () => Promise<void>; browse: () => void; block: (contact: Contact) => Promise<void>;
  } = $props();
  let mailbox = $derived(inboxFor(profile));
  const photos = $derived(inboxPhotos(profile));
  let selected = $derived(contact && !profile.blocks.some(b => b.peer === contact?.peer) &&
    !mailbox.requests.includes(contact.peer) ? contact : null);
  let lines = $derived(mailbox.messages.filter(message => message.peer === selected?.peer));
  let busyPeer = $state(''), error = $state(''), changingPush = $state(false);
  let viewVersion = 0;
  onDestroy(() => { viewVersion++; });
  const mailboxReady = $derived(!!profile.directState || !!profile.directAvailable);
  let sending = $state<Record<string, boolean>>({}), sendErrors = $state<Record<string, string>>({});
  let currentError = $derived((selected && sendErrors[selected.peer]) || error);
  let feedback = $derived(currentError || status || (profile.directState?.delivery_paused ? 'Delivery paused. Refresh to retry.' :
    profile.directState?.unreadable ? 'Some messages couldn’t be opened on this device.' : ''));
  let drafts = $state<Record<string,string>>({});
  let heading = $state<HTMLHeadingElement>();
  let scrollbox = $state<HTMLDivElement>();
  let marked = '';
  $effect(()=>{
    const row=profile.notifications?.messages.find(r=>r.target===selected?.peer && r.unread);
    if (row && lines.some(m=>m.id===row.last) && marked!==row.last) {
      marked=row.last||'';
      void client.readNotification('message',row.target,row.through).catch(()=>{marked='';});
    }
  });
  let lastPeer = '', lastId = '';
  $effect(() => {
    const peer = selected?.peer || '', id = lines.at(-1)?.id || '';
    if (peer && (peer !== lastPeer || id !== lastId)) {
      const list = scrollbox;
      const nearEnd = !list || list.scrollHeight - list.scrollTop - list.clientHeight < 96;
      if (peer !== lastPeer || nearEnd || lines.at(-1)?.outgoing) void tick().then(() => {
        if (scrollbox) scrollbox.scrollTop = scrollbox.scrollHeight;
      });
    }
    lastPeer = peer; lastId = id;
  });

  async function togglePush(kind:string) {
    const enabled = profile.pushCategories||[];
    changingPush=true;error='';
    try {await client.setPushCategories(enabled.includes(kind)?enabled.filter(k=>k!==kind):[...enabled,kind]);}
    catch(failure){error=failure instanceof Error?failure.message:'Couldn’t change notifications.';}
    finally{changingPush=false;}
  }
  function setFolder(next: string) {
    if (next !== tab) { viewVersion++; error = ''; }
    tab = next;
  }
  async function show(person: Contact | null) {
    viewVersion++;
    contact = person;changedContact(person); error = ''; tab = 'chats';
    await tick(); (person ? heading : document.querySelector<HTMLElement>('.dm-tabs [role=tab]'))?.focus();
  }
  function labelFor(peer: string) {
    return profile.posts.flatMap(post => [post,...post.replies]).find(author => author.visibility?.peer === peer)?.author ||
      mailbox.messages.findLast(message => message.peer === peer)?.label || 'Neighbor ' + peer.slice(0,6);
  }
  async function send(person: Contact) {
    if (busyPeer === person.peer || sending[person.peer]) return;
    const body = drafts[person.peer]?.trim();
    if (!body) return;
    sending[person.peer] = true;
    delete sendErrors[person.peer];
    try {
      await client.sendMessage(person, body);
      drafts[person.peer] = '';
      void refresh();
    } catch (reason) {
      const detail = String(reason);
      sendErrors[person.peer] = client.profile?.mailboxElsewhere ? client.otherMessagingDevice : /no available message keys|has not enabled messaging/.test(detail)
        ? 'Ask them to open Yap once, then try sending again.'
        : /too many new conversations/.test(detail) ? 'Try starting this conversation again in an hour.'
        : /queue or label too long/.test(detail) ? 'Your message queue is full. Reconnect and try again.'
        : 'Couldn’t send. Your draft is here. Reconnect and try again.';
    } finally { delete sending[person.peer]; }
  }
  async function accept(peer: string) {
    const version = viewVersion;
    busyPeer = peer; error = '';
    try {
      await client.approveMessage(peer);
      await refresh();
      if (version === viewVersion && !contact && tab === 'requests') await show({peer,label:labelFor(peer)});
    } catch {
      if (version === viewVersion && !contact) error = client.profile?.mailboxElsewhere ? client.otherMessagingDevice : 'Couldn’t accept this request. Reconnect and try again.';
    } finally { busyPeer = ''; }
  }
  async function blockPerson(person: Contact) {
    if (!window.confirm('Block '+person.label+'? This hides their posts and stops messages between you.')) return;
    const version = viewVersion, originalPeer = contact?.peer;
    busyPeer = person.peer; error = '';
    try {
      await block(person);
      if (version !== viewVersion || contact?.peer !== originalPeer) return;
      if (originalPeer) await show(null);
      else document.querySelector<HTMLElement>('.dm-tabs [aria-selected=true]')?.focus();
    } catch {
      if (version === viewVersion && contact?.peer === originalPeer) error = 'Couldn’t save this block. Try again.';
    } finally { busyPeer = ''; }
  }
</script>

{#if selected}
  <div class="dm-top"><ActionButton onclick={() => show(null)}><AppIcon name="arrowLeft"/>Back to inbox</ActionButton>
    <ActionButton disabled={!!busyPeer} onclick={() => selected && blockPerson(selected)}>Block</ActionButton></div>
  <div class="recipient">
    <Avatar label={selected.label} photo={photos.get(selected.peer)}/>
    <div><h1 class="dm-title" tabindex="-1" bind:this={heading}>{selected.label}</h1>
      <p class="dm-meta">End-to-end encrypted</p></div>
  </div>
  <div class="dm-history" data-scroll-history role="log" aria-label="Messages" aria-relevant="additions" bind:this={scrollbox}>
    {#each lines as message (message.id)}
      <div class="dm-line" class:outgoing={message.outgoing}>
        <div class="message-content"><span class="sr-only">{message.outgoing ? 'You' : selected.label}</span><p>{message.body}</p>
        {#if !message.outgoing}
          <ContentMenu label="Message options" items={[
            { label: 'Report message', run: () => report('message', message.peer, message.body) }
          ]}/>
        {/if}</div>
        {#if message.outgoing}<small>{message.status === 'sent' ? 'Sent' : message.status === 'queued' ? 'Waiting to send' : 'Couldn’t deliver'}</small>{/if}
      </div>
    {:else}
      {#if mailboxReady}<p class="dm-meta">Send your first message to {selected.label}.</p>
      {:else if !feedback}<p class="dm-meta" role="status">Loading inbox…</p>{/if}
    {/each}
  </div>
  {#if feedback}
    <div class="dm-feedback-anchor">
      <span class="sr-only" role="status">{feedback}</span>
      <details class="delivery-feedback" use:deliveryDisclosure={() => document.getElementById('dm-body')?.focus({preventScroll: true})}>
        <summary>{currentError ? 'Message needs attention' : status.startsWith('Offline inbox') ? 'Offline inbox' : 'Inbox status'}</summary>
        <p>{feedback}</p>
      </details>
    </div>
  {/if}
  <form class="dm-compose" onsubmit={event => {event.preventDefault(); if (selected) void send(selected);}}>
    <label class="sr-only" for="dm-body">Message</label>
    <textarea placeholder="Write a message…" id="dm-body" rows="2" maxlength="1000" value={drafts[selected.peer] || ''} readonly={busyPeer === selected.peer || sending[selected.peer]}
      onfocus={event => event.currentTarget.scrollIntoView({block: 'nearest'})}
      oninput={event => { if (selected) drafts[selected.peer] = event.currentTarget.value; }}></textarea>
    <ActionButton variant="filled" disabled={busyPeer === selected.peer || sending[selected.peer] || !profile.directAvailable || !drafts[selected.peer]?.trim()}>{sending[selected.peer] ? 'Sending…' : 'Send'}</ActionButton>
  </form>
{:else}
  <Tabs value={tab} onValueChange={({ value }) => setFolder(value)}>
    <Tabs.List class="dm-tabs" aria-label="Inbox folders">
      <Tabs.Trigger class="btn" value="activity">Activity</Tabs.Trigger>
      <Tabs.Trigger class="btn" value="chats">Messages</Tabs.Trigger>

    </Tabs.List>
    <div use:pageMotion={tab === 'chats' ? 0 : 1}>
    <Tabs.Content value="activity">
      {#each (profile.notifications?.activity||[]).filter(item=>!profile.muted?.[item.target]) as item (item.target)}
        <button class="dm-thread" onclick={()=>openActivity(item)}>
          <AppIcon name="conversation"/><span class="conversation"><strong>{item.title}</strong>
            <span>{item.unread ? item.unread+(item.unread===1?' new reply':' new replies') : 'Read'}</span></span><AppIcon name="arrowRight"/>
        </button>
      {:else}<section class="dm-empty"><h2>No replies yet</h2><p>Replies to your posts will appear here.</p></section>{/each}
    </Tabs.Content>
    <Tabs.Content value="chats">
      {#if mailbox.requests.length}<button class="dm-thread" onclick={()=>setFolder('requests')}><strong>Message requests ({mailbox.requests.length})</strong><AppIcon name="arrowRight"/></button>{/if}
      {#each mailbox.threads as thread (thread.peer)}
        <button class="dm-thread" onclick={() => show({peer:thread.peer,label:thread.label})}>
          <Avatar label={thread.label} photo={photos.get(thread.peer)}/>
          <span class="conversation"><strong>{thread.label}</strong>{#if profile.notifications?.messages.some(row=>row.target===thread.peer && row.unread)}<small>Unread</small>{/if}<span>{thread.outgoing ? 'You: ' : ''}{thread.body}</span></span>
          <AppIcon name="arrowRight"/>
        </button>
      {:else}
        {#if mailboxReady && !profile.mailboxElsewhere}<section class="dm-empty"><span class="empty-icon"><AppIcon name="conversation" /></span><h2 tabindex="-1" bind:this={heading}>No conversations yet</h2>
          <p>Select a neighbor’s name on a post or reply, then choose Message on their profile.</p>
          <ActionButton onclick={browse}>Browse posts</ActionButton></section>{/if}
      {/each}
    </Tabs.Content>
    <Tabs.Content value="requests">
      <ActionButton onclick={()=>setFolder('chats')}>Back to messages</ActionButton>
      {#each mailbox.requests as peer (peer)}
        <div class="dm-request"><div class="requester"><Avatar label={labelFor(peer)} photo={photos.get(peer)}/><strong>{labelFor(peer)}</strong></div><div class="actions">
          <ActionButton variant="tonal" disabled={!!busyPeer || !profile.directAvailable} onclick={() => accept(peer)}>Accept</ActionButton>
          <ActionButton disabled={!!busyPeer} onclick={() => blockPerson({peer,label:labelFor(peer)})}>Block</ActionButton>
          <ActionButton disabled={!!busyPeer} onclick={async()=>{try{await client.dismissRequest(peer)}catch{error='Couldn’t dismiss the request. Try again.'}}}>Dismiss</ActionButton>
          <ActionButton onclick={() => report('account',peer,'')}>Report account</ActionButton>
        </div></div>
      {:else}{#if mailboxReady && !profile.mailboxElsewhere}<section class="dm-empty"><h2>No message requests</h2><p>Messages that need your approval will appear here.</p></section>{/if}{/each}
    </Tabs.Content>
    </div>
  </Tabs>
  {#if profile.pushCapabilities?.webKey && typeof PushManager!=='undefined'}
    <details class="delivery-feedback"><summary>Device notifications</summary>
      {#each [['reply','Replies to my posts'],['message','Direct messages'],['request','Message requests']] as option}
        <label class="push-option"><input type="checkbox" disabled={changingPush} checked={profile.pushCategories?.includes(option[0])||false}
          onchange={()=>togglePush(option[0])}/>{option[1]}</label>
      {/each}
    </details>
  {/if}
  {#if error && !selected}<p role="alert">{error}</p>{/if}
{/if}
{#if !selected && feedback}<p class="dm-feedback" role="status">{feedback}</p>
{:else if !selected && !mailboxReady && !feedback}<p class="dm-feedback" role="status">Loading inbox…</p>{/if}


<style>
  .push-option {display:flex;align-items:center;gap:.6rem;min-height:2.75rem;}
  .dm-thread { display: grid; grid-template-columns: auto minmax(0,1fr) auto; align-items: center; gap: .85rem; padding: 1rem .35rem; }
  .dm-thread .conversation { display: grid; min-width: 0; gap: .16rem; }
  .conversation strong { color: var(--ink); font: var(--type-label); overflow: hidden; text-overflow: ellipsis; }
  .conversation span { font: var(--type-small); }
  .dm-thread :global(> svg) { width: 17px; color: var(--quiet); }
  .dm-thread :global(.avatar) { color: var(--on-accent); }
  .recipient, .requester { display: flex; gap: .75rem; align-items: center; }
  .recipient { padding: .5rem 0 .85rem; border-bottom: 1px solid var(--line); }
  .recipient > div { min-width: 0; }
  .recipient .dm-title { margin: 0; font: var(--type-post-title); }
  .recipient .dm-meta { margin: .15rem 0 0; }
  .dm-empty h2 { font: var(--type-post-title); }
  .dm-empty .empty-icon { width: 3.2rem; height: 3.2rem; border-radius: 36% 44% 39% 27%; }
  .dm-request { gap: .75rem; }
  .dm-request .actions { gap: .3rem; }
  @media (prefers-reduced-motion: no-preference) {
    .dm-thread :global(> svg) { transition: transform 140ms ease-out; }
    .dm-thread:focus-visible :global(> svg) { transform: translateX(3px); }
    @media (hover: hover) { .dm-thread:hover :global(> svg) { transform: translateX(3px); } }
  }
</style>
