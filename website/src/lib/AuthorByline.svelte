<script lang="ts">
  import ActionButton from './ActionButton.svelte';
  import ProfileSummary from './ProfileSummary.svelte';
  import Sheet from './Sheet.svelte';
  import { authorLabel, isAnonymous, type Author } from './community';

  let { author, message, loadPortrait }: { author: Author; message?: () => void; loadPortrait?: () => Promise<string | null> } = $props();
  const label = $derived(authorLabel(author));
  let portrait = $state<string | null>(null);
  let request = 0;

  function prepare() {
    const pending = ++request;
    if (!author.profile?.photo || !loadPortrait) return;
    void loadPortrait().then(photo => { if (request === pending) portrait = photo; }).catch(() => {});
  }

  function dismiss() { request++; portrait = null; }
</script>

<div class="byline post-byline">
  {#if isAnonymous(author)}
    <strong>{label}</strong>
  {:else}
    {#snippet messageAction(close: () => void)}
      <ActionButton variant="filled" onclick={() => { close(); message?.(); }}>Message {label}</ActionButton>
    {/snippet}
    <Sheet actions={message ? messageAction : undefined} {label} title={label} triggerClass="byline-name" appearance="profile" {prepare} {dismiss}>
      {#snippet opener()}
        <span>{label}</span>
        {#if author.username}<span class="handle">{author.username}</span>{/if}
      {/snippet}
      {#snippet children()}
        <ProfileSummary name={label} username={author.username} details={author.profile && portrait ? {...author.profile,photo:portrait} : author.profile}/>
      {/snippet}
    </Sheet>
  {/if}
</div>
