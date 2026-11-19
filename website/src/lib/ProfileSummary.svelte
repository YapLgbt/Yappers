<script lang="ts">
  import Avatar from './Avatar.svelte';
  import type { PublicProfile } from './community';
  import type { Snippet } from 'svelte';

  let { name, username, details, own = false, children }: {
    name: string; username?: string | null; details?: PublicProfile | null; own?: boolean; children?: Snippet;
  } = $props();
  const sharedDetails = $derived(details?.photo || details?.pronouns || details?.bio ||
    details?.identity || details?.interests || details?.hereFor?.length);
</script>

<div class="profile-summary" class:own>
  <div class="identity">
    <Avatar label={name} photo={details?.photo} large />
    <div class="identity-text">
      {#if own}<h1>{name}</h1>{/if}
      {#if username}<p class="username">{username}</p>{/if}
      {#if details?.pronouns}<p>{details.pronouns}</p>{/if}
      {#if children}{@render children()}{/if}
    </div>
  </div>
  {#if details?.bio}<p class="bio">{details.bio}</p>{/if}
  {#if details?.identity || details?.interests || details?.hereFor?.length}
    <dl>
      {#each [['Identity', details.identity], ['Interests', details.interests], ['Here for', [...new Set(details.hereFor)].join(', ')]] as [label, value]}
        {#if value}<div><dt>{label}</dt><dd>{value}</dd></div>{/if}
      {/each}
    </dl>
  {/if}
  {#if !own}
    <p class="source">{sharedDetails ? 'Shared with this post' : 'No further profile details shared with this post.'}</p>
  {/if}
</div>

<style>
  .profile-summary { overflow-wrap: anywhere; }
  .profile-summary.own { margin-block: .5rem 1.25rem; }
  .own::before {
    display: block;
    height: clamp(2.5rem, 7vw, 4.75rem);
    border-radius: .875rem .875rem 0 0;
    background: linear-gradient(100deg, color-mix(in srgb, var(--plum) 70%, transparent), transparent 75%),
      var(--accent) url('/app/estuary.webp') center 53% / cover;
    content: '';
  }
  .identity { display: flex; gap: 1rem; align-items: center; }
  .own .identity { align-items: flex-end; margin-top: -1.45rem; padding-inline: .65rem; }
  .own .identity-text { padding-top: 2rem; }
  .own .identity :global(.avatar) { outline: 4px solid var(--bg); }
  h1 { margin: 0; font: var(--type-page-title); letter-spacing: -.04em; }
  .identity-text { min-width: 0; }
  .identity-text p, .source { margin: .12rem 0; color: var(--quiet); font: var(--type-small); }
  .identity-text .username { font-weight: 600; }
  .bio { margin: 1rem 0; max-width: 52ch; font: var(--type-post-body); white-space: pre-wrap; }
  dl { margin: 1rem 0 0; border-top: 1px solid var(--line); }
  dl div { display: grid; grid-template-columns: minmax(4.5rem, 25%) minmax(0, 1fr); gap: .5rem; padding-block: .55rem; border-bottom: 1px solid var(--line); }
  dt { color: var(--quiet); font: var(--type-small); }
  dd { margin: 0; font: var(--type-label); white-space: pre-wrap; }
  .source { margin-top: 1rem; }
</style>
