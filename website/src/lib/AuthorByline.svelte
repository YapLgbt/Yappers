<script lang="ts">
  import ActionButton from './ActionButton.svelte';
  import ProfileSummary from './ProfileSummary.svelte';
  import Sheet from './Sheet.svelte';
  import { authorLabel, isAnonymous, type Author } from './community';

  let { author, message }: { author: Author; message?: () => void } = $props();
  const label = $derived(authorLabel(author));
</script>

<div class="byline post-byline">
  {#if isAnonymous(author)}
    <strong>{label}</strong>
  {:else}
    {#snippet messageAction(close: () => void)}
      <ActionButton variant="filled" onclick={() => { close(); message?.(); }}>Message {label}</ActionButton>
    {/snippet}
    <Sheet actions={message ? messageAction : undefined} {label} title={label} triggerClass="byline-name">
      {#snippet opener()}
        <span>{label}</span>
        {#if author.username}<span class="handle">{author.username}</span>{/if}
      {/snippet}
      {#snippet children()}
        <ProfileSummary name={label} username={author.username} details={author.profile}/>
      {/snippet}
    </Sheet>
  {/if}
</div>
