<script lang="ts">
  import ActionButton from './ActionButton.svelte';
  import AppIcon from './AppIcon.svelte';
  import ContentMenu from './ContentMenu.svelte';
  import { tick } from 'svelte';
  import { Tabs } from 'bits-ui';
  import { inboxFor, type Community, type Profile, type Contact } from './community';

  let { profile, client, contact = $bindable(null), status, refresh, browse, block, report, changedContact }: {
    changedContact:(person:Contact|null)=>void;report: (kind:"message"|"account",id:string,text:string)=>void; profile: Profile; client: Community; contact: Contact | null; status: string;
    refresh: () => Promise<void>; browse: () => void; block: (contact: Contact) => Promise<void>;
  } = $props();
  let mailbox = $derived(inboxFor(profile));
  let selected = $derived(contact && !profile.blocks.some(b => b.peer === contact?.peer) &&
    !mailbox.requests.includes(contact.peer) ? contact : null);
  let lines = $derived(mailbox.messages.filter(message => message.peer === selected?.peer));
  let tab = $state('chats'), busy = $state(false), error = $state('');
  let drafts = $state<Record<string,string>>({});
  let heading = $state<HTMLHeadingElement>();
  let scrollbox = $state<HTMLDivElement>();
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

  async function show(person: Contact | null) {
    contact = person;changedContact(person); error = ''; tab = 'chats';
    await tick(); (person ? heading : document.querySelector<HTMLElement>('.dm-tabs [role=tab]'))?.focus();
  }
  function labelFor(peer: string) {
    return profile.posts.flatMap(post => [post,...post.replies]).find(author => author.visibility?.peer === peer)?.author ||
      mailbox.messages.findLast(message => message.peer === peer)?.label || 'Neighbor ' + peer.slice(0,6);
  }
  async function send(person: Contact) {
    if (busy) return;
    const body = drafts[person.peer]?.trim();
    if (!body) return;
    busy = true; error = '';
    try {
      await client.sendMessage(person, body);
      drafts[person.peer] = '';
      void refresh();
    } catch (reason) {
      const detail = String(reason);
      error = client.profile?.mailboxElsewhere ? client.otherMessagingDevice : /no available message keys|has not enabled messaging/.test(detail)
        ? 'Ask them to open Yap once, then try sending again.'
        : /too many new conversations/.test(detail) ? 'Try starting this conversation again in an hour.'
        : /queue or label too long/.test(detail) ? 'Your message queue is full. Reconnect and try again.'
        : 'Couldn’t send. Your draft is here. Reconnect and try again.';
    } finally { busy = false; }
  }
  async function accept(peer: string) {
    busy = true; error = '';
    try {
      await client.approveMessage(peer);
      await refresh();
      await show({peer,label:labelFor(peer)});
    } catch { error = client.profile?.mailboxElsewhere ? client.otherMessagingDevice : 'Couldn’t accept this request. Reconnect and try again.'; }
    finally { busy = false; }
  }
  async function blockPerson(person: Contact) {
    if (!window.confirm('Block '+person.label+'? This hides their posts and stops messages between you.')) return;
    busy = true; error = '';
    try { await block(person); await show(null); }
    catch { error = 'Couldn’t save this block. Try again.'; }
    finally { busy = false; }
  }
</script>

{#if selected}
  <div class="dm-top"><ActionButton onclick={() => show(null)}>Back to inbox</ActionButton>
    <ActionButton disabled={busy} onclick={() => selected && blockPerson(selected)}>Block</ActionButton></div>
  <h2 class="dm-title" tabindex="-1" bind:this={heading}>{selected.label}</h2>
  <p class="dm-meta">End-to-end encrypted</p>
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
    {:else}<p class="dm-meta">Send your first message to {selected.label}.</p>{/each}
  </div>
  <form class="dm-compose" onsubmit={event => {event.preventDefault(); if (selected) void send(selected);}}>
    <label for="dm-body">Message</label>
    <textarea id="dm-body" rows="2" maxlength="1000" value={drafts[selected.peer] || ''} readonly={busy}
      oninput={event => { if (selected) drafts[selected.peer] = event.currentTarget.value; }}></textarea>
    <ActionButton variant="filled" disabled={busy || !profile.directAvailable || !drafts[selected.peer]?.trim()}>Send</ActionButton>
  </form>
{:else}
  <Tabs.Root bind:value={tab}>
    <Tabs.List class="dm-tabs mtrl-tabs mtrl-tabs--primary" aria-label="Inbox folders">
      <Tabs.Trigger class="mtrl-button mtrl-tab mtrl-tab--text-only" value="chats">Conversations</Tabs.Trigger>
      <Tabs.Trigger class="mtrl-button mtrl-tab mtrl-tab--text-only" value="requests">Requests{mailbox.requests.length ? ' ('+mailbox.requests.length+')' : ''}</Tabs.Trigger>
    </Tabs.List>
    <Tabs.Content value="chats">
      {#each mailbox.threads as thread (thread.peer)}
        <button class="dm-thread" onclick={() => show({peer:thread.peer,label:thread.label})}>
          <strong>{thread.label}</strong><span>{thread.outgoing ? 'You: ' : ''}{thread.body}</span>
        </button>
      {:else}
        {#if !profile.mailboxElsewhere}<section class="dm-empty"><span class="empty-icon"><AppIcon name="nearby" /></span><h2 tabindex="-1" bind:this={heading}>No conversations yet</h2>
          <p>Select a neighbor’s name on a post or reply, then choose Message on their profile.</p>
          <ActionButton onclick={browse}>Browse posts</ActionButton></section>{/if}
      {/each}
    </Tabs.Content>
    <Tabs.Content value="requests">
      {#each mailbox.requests as peer (peer)}
        <div class="dm-request"><strong>{labelFor(peer)}</strong><div class="actions">
          <ActionButton onclick={() => report('account',peer,'')}>Report account</ActionButton>
          <ActionButton disabled={busy || !profile.directAvailable} onclick={() => accept(peer)}>Accept</ActionButton>
          <ActionButton disabled={busy} onclick={() => blockPerson({peer,label:labelFor(peer)})}>Block</ActionButton>
        </div></div>
      {:else}{#if !profile.mailboxElsewhere}<section class="dm-empty"><span class="empty-icon"><AppIcon name="nearby" /></span><h2>No message requests</h2><p>Messages that need your approval will appear here.</p></section>{/if}{/each}
    </Tabs.Content>
  </Tabs.Root>
{/if}
{#if error || status || profile.directState?.delivery_paused || profile.directState?.unreadable}
  <p class="dm-feedback" role="status">{error || status || (profile.directState?.delivery_paused ? 'Delivery paused. Refresh to retry.' : 'Some messages couldn’t be opened on this device.')}</p>
{/if}
