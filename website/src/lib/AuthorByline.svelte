<script lang="ts">
  import ActionButton from './ActionButton.svelte';
  import Avatar from './Avatar.svelte';
  import Sheet from './Sheet.svelte';
  import { authorLabel, isAnonymous, type Author } from './community';

  let { author, message }: { author: Author; message?: () => void } = $props();
  const label = $derived(authorLabel(author));
  const details = $derived(author.profile);
</script>

<div class="byline post-byline">
  {#if isAnonymous(author)}
    <strong>{label}</strong>
  {:else}
    <Sheet {label} title={label} triggerClass="byline-name">
      {#snippet children(close)}
        <div class="author-profile">
          {#if details?.pronouns}<p>{details.pronouns}</p>{/if}
          {#if author.username}<p class="handle">{author.username}</p>{/if}
          {#if details}<p class="post-meta">Shared with this post</p>{/if}
          {#if details?.bio}<p class="author-bio">{details.bio}</p>{/if}
          {#if details?.hereFor?.length}
            <h3>Here for</h3>
            <ul class="author-tags">
              {#each [...new Set(details.hereFor)] as tag (tag)}<li>{tag}</li>{/each}
            </ul>
          {/if}
          {#if details?.identity}<p>{details.identity}</p>{/if}
          {#if details?.interests}<p>{details.interests}</p>{/if}
          {#if details?.photo}<Avatar photo={details.photo} {label} large />{/if}
        </div>
        {#if message}<ActionButton variant="tonal" onclick={() => { close(); message?.(); }}>Message {label}</ActionButton>{/if}
      {/snippet}
    </Sheet>
    {#if author.username}<span class="handle">{author.username}</span>{/if}
  {/if}
</div>
