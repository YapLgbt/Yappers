<script lang="ts">
  import { onMount } from 'svelte';
  import { errorMessage, type Community } from './community';
  import ActionButton from './ActionButton.svelte';
  import Sheet from './Sheet.svelte';

  type Summary = { id: string; name: string };
  type Post = { id: string; author: string; headline: string | null; body: string; parent: string | null };
  type Page = {
    name: string; owner: boolean; phase: 'pending' | 'joined' | 'removed';
    posts: Post[]; pending: boolean; before: number | null;
    joinRequest: string | null; invitation: string | null;
    members: Record<string, { since: number }> | null;
  };
  type Editor = 'Create private group' | 'Open invitation' | 'Approve a device' | 'New post' | 'Write a reply';

  let { client, alias, onBack }: { client: Community; alias: string; onBack: () => void } = $props();
  let groups = $state<Summary[]>([]);
  let group = $state<string | null>(null);
  let page = $state<Page | null>(null);
  let editor = $state<Editor | null>(null);
  let parent = $state<Post | null>(null);
  let input = $state('');
  let headline = $state('');
  let busy = $state(false);
  let error = $state('');
  let status = $state('');
  let heading: HTMLHeadingElement;

  const roots = $derived(page?.posts.filter(post => post.parent === null) || []);
  const rootIds = $derived(new Set(roots.map(post => post.id)));
  const orphaned = $derived(page?.posts.filter(post => post.parent && !rootIds.has(post.parent)) || []);
  const members = $derived(Object.entries(page?.members || {}).filter(([, member]) => member.since !== 1));
  const post = $derived(editor === 'New post');
  const reply = $derived(editor === 'Write a reply');
  const label = $derived(editor === 'Create private group' ? 'Group name'
    : editor === 'Open invitation' ? 'Paste invitation'
    : editor === 'Approve a device' ? 'Paste signed join request'
    : post ? 'Post body' : 'Reply');

  function request<T>(kind: string, fields: Record<string, unknown> = {}) {
    return client.privateGroupAction<T>({ kind, ...fields });
  }
  async function run(action: () => Promise<void>) {
    if (busy) return;
    busy = true;
    error = '';
    status = '';
    try { await action(); }
    catch (failure) { error = errorMessage(failure); }
    finally { busy = false; }
  }
  async function list() {
    groups = (await request<Summary[]>('list')).sort((a, b) => a.name.localeCompare(b.name));
  }
  async function open(id: string, before: number | null = null) {
    const next = await request<Page>('open', { group: id, before });
    if (before && page && next.phase === 'joined') {
      next.posts = [...new Map([...page.posts, ...next.posts].map(post => [post.id, post])).values()];
    }
    page = next;
    group = id;
  }
  function begin(kind: Editor, responseTo: Post | null = null) {
    editor = kind;
    input = '';
    headline = '';
    parent = responseTo;
    error = '';
    status = '';
    queueMicrotask(() => heading?.focus());
  }
  function back() {
    if (editor) editor = null;
    else if (group) { group = null; page = null; }
    else { onBack(); return; }
    error = '';
    status = '';
    queueMicrotask(() => heading?.focus());
  }
  async function copy(value: string, label: string) {
    await navigator.clipboard.writeText(value);
    status = label;
  }
  async function publish(id: string) {
    await request('publish', { group: id });
    if (page) page.pending = false;
    await open(id);
  }
  function submit(event: SubmitEvent) {
    event.preventDefault();
    void run(async () => {
      if (editor === 'Create private group' || editor === 'Open invitation') {
        const id = editor === 'Create private group'
          ? await request<string>('create', { name: input.trim() })
          : await request<string>('import', { invitation: input.trim() });
        await list();
        editor = null;
        await open(id);
      } else if (group && editor === 'Approve a device') {
        await request('approve', { group, request: input.trim() });
        editor = null;
        await open(group);
      } else if (group && (post || reply)) {
        await request('prepare', { group, draft: {
          author: alias, headline: post ? headline.trim() : null, body: input.trim(), parent: parent?.id ?? null,
        } });
        if (page) page.pending = true;
        editor = null;
        input = '';
        headline = '';
        parent = null;
        await publish(group);
      }
    });
  }

  onMount(() => { void run(list); });
</script>

<section class="private-groups" aria-labelledby="private-group-heading" aria-busy={busy}>
  <header>
    <ActionButton disabled={busy} onclick={back}>{group || editor ? 'Back' : 'All groups'}</ActionButton>
    <h1 id="private-group-heading" tabindex="-1" bind:this={heading}>{editor || page?.name || 'Private groups'}</h1>
  </header>
  {#if error}<p role="alert">{error}</p>{/if}
  {#if status}<p role="status">{status}</p>{/if}
  {#if busy}<p role="status">Working…</p>{/if}

  {#if editor}
    <form onsubmit={submit}>
      {#if post}
        <label>Headline <input required maxlength="100" bind:value={headline} disabled={busy} /></label>
      {:else if reply}
        <p>Replying to {parent?.headline || 'a post'}</p>
      {/if}
      {#if editor === 'Create private group'}
        <label>{label} <input required maxlength="60" bind:value={input} disabled={busy} /></label>
      {:else}
        <label>{label} <textarea required maxlength={post || reply ? 3000 : 4096}
          rows={post || reply ? 6 : 4} bind:value={input} disabled={busy}></textarea></label>
      {/if}
      {#if editor === 'Approve a device'}
        <p>Approve only a request you received directly from the person you invited.</p>
      {:else if editor === 'Open invitation'}
        <p>The owner will need to approve this device before you can read or post.</p>
      {/if}
      <ActionButton type="submit" variant="filled" disabled={busy || !input.trim() || (post && !headline.trim())}>
        {post ? 'Post' : reply ? 'Send reply' : editor === 'Approve a device' ? 'Approve device' : editor}
      </ActionButton>
    </form>
  {:else if group && page}
    {@const id = group}
    {@const current = page}
    <p>{current.phase === 'pending' ? 'Send your join request to the owner. Your history starts when they approve it.'
      : current.phase === 'removed' ? 'This device no longer has access.'
      : 'Posts and replies are end-to-end encrypted.'}</p>
    <div class="actions">
      {#if current.phase === 'joined'}
        <ActionButton variant="filled" disabled={busy || current.pending} onclick={() => begin('New post')}>New post</ActionButton>
      {/if}
      <ActionButton disabled={busy} onclick={() => run(() => open(id))}>Refresh group</ActionButton>
      {#if current.joinRequest}
        {@const code = current.joinRequest}
        <ActionButton variant="tonal" disabled={busy} onclick={() => run(() => copy(code, 'Join request copied.'))}>
          Copy join request
        </ActionButton>
      {/if}
      {#if current.invitation}
        {@const code = current.invitation}
        <ActionButton disabled={busy} onclick={() => run(() => copy(code, 'Invitation copied.'))}>Copy invitation</ActionButton>
        <ActionButton disabled={busy} onclick={() => begin('Approve a device')}>Approve a device</ActionButton>
      {/if}
    </div>

    {#if current.pending}
      <div class="actions">
        <ActionButton variant="tonal" disabled={busy || current.phase !== 'joined'} onclick={() => run(async () => {
          await request('prepare', { group: id });
          await publish(id);
        })}>Retry pending post</ActionButton>
        <Sheet title="Discard the pending post?" label="Discard pending post" {busy}>
          {#snippet children()}<p>The saved draft will be removed from this device.</p>{/snippet}
          {#snippet actions(close)}
            <ActionButton variant="filled" disabled={busy} onclick={() => run(async () => {
              await request('discard', { group: id });
              if (page) page.pending = false;
              close();
            })}>Discard draft</ActionButton>
          {/snippet}
        </Sheet>
      </div>
    {/if}

    {#if current.phase === 'joined'}
      <div class="threads">
        {#each roots as root (root.id)}
          <article>
            <h2>{root.headline}</h2>
            <p class="byline">{root.author}</p>
            <p class="body">{root.body}</p>
            {#each current.posts.filter(post => post.parent === root.id).toReversed() as comment (comment.id)}
              <div class="reply">
                <p class="byline">{comment.author}</p>
                <p class="body">{comment.body}</p>
              </div>
            {/each}
            <ActionButton disabled={busy || current.pending} onclick={() => begin('Write a reply', root)}>Reply</ActionButton>
          </article>
        {/each}
        {#each orphaned as comment (comment.id)}
          <article>
            <h2>Reply to an earlier post</h2>
            <p class="byline">{comment.author}</p>
            <p class="body">{comment.body}</p>
          </article>
        {/each}
        {#if !current.posts.length}<p>No posts since you joined.</p>{/if}
        {#if current.before}
          <ActionButton disabled={busy} onclick={() => run(() => open(id, current.before))}>Load older posts</ActionButton>
        {/if}
      </div>
      {#if current.owner && members.length}
        <h2>Member devices</h2>
        {#each members as [device] (device)}
          <Sheet title="Remove this device?" label={`Remove device ${device.slice(0, 12)}`} {busy}>
            {#snippet children()}<p>Future posts and replies will use new keys.</p>{/snippet}
            {#snippet actions(close)}
              <ActionButton variant="filled" disabled={busy} onclick={() => run(async () => {
                await request('remove', { group: id, device });
                close();
                await open(id);
              })}>Remove device</ActionButton>
            {/snippet}
          </Sheet>
        {/each}
      {/if}
    {/if}
  {:else}
    <div class="actions">
      <ActionButton variant="filled" disabled={busy} onclick={() => begin('Create private group')}>Create private group</ActionButton>
      <ActionButton variant="tonal" disabled={busy} onclick={() => begin('Open invitation')}>Open invitation</ActionButton>
    </div>
    {#if groups.length}
      <ul class="group-list">
        {#each groups as saved (saved.id)}
          <li><ActionButton disabled={busy} onclick={() => run(() => open(saved.id))}>{saved.name}</ActionButton></li>
        {/each}
      </ul>
    {/if}
  {/if}
</section>

<style>
  .private-groups { max-inline-size: 68ch; margin-inline: auto; }
  header, form, label { display: grid; justify-items: start; gap: .6rem; }
  header { margin-block-end: 1rem; }
  header h1 { margin: 0; }
  label { inline-size: 100%; }
  input, textarea { inline-size: 100%; padding: .65rem; font: inherit; border: 1px solid var(--line); border-radius: .5rem; }
  textarea { resize: vertical; }
  .actions { display: flex; flex-wrap: wrap; gap: .5rem; margin-block: .75rem; }
  .group-list { list-style: none; padding: 0; }
  .threads { margin-block: 1.25rem; }
  article { border-block-start: 1px solid var(--line); padding-block: .85rem; overflow-wrap: anywhere; }
  article h2 { margin-block: 0 .4rem; font-size: 1.15rem; }
  .byline { margin-block: .25rem; font-weight: 600; }
  .body { white-space: pre-wrap; margin-block: .35rem .85rem; }
  .reply { margin-inline-start: .75rem; border-block-start: 1px solid var(--line); padding-block-start: .5rem; }
</style>
