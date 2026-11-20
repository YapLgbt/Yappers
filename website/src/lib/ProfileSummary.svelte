<script lang="ts">
  import Avatar from './Avatar.svelte';
  import type { PublicProfile } from './community';
  import type { Snippet } from 'svelte';

  let { name, username, details, own = false, children }: {
    name: string; username?: string | null; details?: PublicProfile | null; own?: boolean; children?: Snippet;
  } = $props();
  const intentions = $derived([...new Set(details?.hereFor ?? [])]);
  const sharedDetails = $derived(details?.photo || details?.pronouns || details?.bio ||
    details?.identity || details?.interests || intentions.length);
</script>

<div class="profile-summary" class:own>
  <Avatar label={name} photo={details?.photo} portrait />
  <div class="profile-copy">
    <div class="profile-nameplate">
      {#if own && !details?.photo}
        <span class="default-portrait"><Avatar label={name} photo="" large /></span>
      {/if}
      <div>
        <svelte:element this={own ? 'h1' : 'h2'} class="profile-name">{name}</svelte:element>
        {#if details?.pronouns || username}
          <div class="profile-byline">
            {#if details?.pronouns}<span>{details.pronouns}</span>{/if}
            {#if username}<span class="profile-username">{username}</span>{/if}
          </div>
        {/if}
        {#if children}{@render children()}{/if}
      </div>
    </div>
    {#if intentions.length}
      <div class="profile-intentions-display">
        <h3>Here for</h3>
        <ul role="list">
          {#each intentions as intention (intention)}<li>{intention}</li>{/each}
        </ul>
      </div>
    {/if}
    {#if details?.bio}<p class="profile-bio">{details.bio}</p>{/if}
    {#if details?.identity || details?.interests}
      <dl>
        {#if details.identity}<div><dt>Identity</dt><dd>{details.identity}</dd></div>{/if}
        {#if details.interests}<div><dt>Current joys</dt><dd>{details.interests}</dd></div>{/if}
      </dl>
    {/if}
    {#if !own}
      <p class="profile-source">{sharedDetails ? 'Current public profile' : 'No further public profile details.'}</p>
    {/if}
  </div>
</div>

<style>
  .profile-summary {
    display: grid;
    align-items: start;
    gap: .875rem;
    overflow-wrap: anywhere;
  }
  .profile-summary.own { margin-block: .5rem 1.25rem; }
  .profile-summary > :global(.portrait) { width: min(100%, 24rem); }
  .profile-copy { min-width: 0; }
  .profile-nameplate { display: flex; align-items: center; flex-wrap: wrap; gap: .875rem; }
  .profile-nameplate > div { flex: 1 1 9rem; min-width: 0; }
  .default-portrait {
    display: grid;
    place-items: center;
    width: 5.5rem;
    aspect-ratio: 1;
    border-radius: var(--corner-large);
    background: var(--accent) url('/app/estuary.webp') center / cover;
  }
  .default-portrait :global(.avatar) { width: 4.125rem; height: 4.125rem; }
  .profile-name {
    margin: 0;
    font: 700 2.125rem/1.08 var(--sans);
    letter-spacing: -.035em;
  }
  .profile-byline {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: .125rem .75rem;
    margin-block-start: .375rem;
    color: var(--quiet);
    font: var(--type-small);
  }
  .profile-username { color: var(--plum); }
  .profile-intentions-display { margin-block-start: .875rem; }
  h3, dt {
    margin: 0;
    color: var(--quiet);
    font: var(--type-label);
  }
  ul { display: flex; flex-wrap: wrap; gap: .375rem; margin: .375rem 0 0; padding: 0; list-style: none; }
  li {
    padding: .3rem .65rem;
    border-radius: var(--corner-full);
    background: var(--accent);
    color: var(--on-accent);
    font: var(--type-label);
  }
  .profile-bio { margin: .875rem 0 0; max-width: 54ch; font: var(--type-post-body); white-space: pre-wrap; }
  dl { display: grid; gap: .75rem; margin: .875rem 0 0; }
  dd { margin: .125rem 0 0; font: var(--type-body); white-space: pre-wrap; }
  .profile-source { margin: 1rem 0 0; color: var(--quiet); font: var(--type-caption); }

  @container (min-width: 34rem) {
    .profile-summary:has(> :global(.has-photo)) { grid-template-columns: minmax(0, 5fr) minmax(0, 6fr); gap: 1.5rem; }
  }
</style>
