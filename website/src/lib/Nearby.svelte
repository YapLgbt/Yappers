<script lang="ts">
  import { tick } from 'svelte';
  import ActionButton from './ActionButton.svelte';
  import Sheet from './Sheet.svelte';
  import AppIcon from './AppIcon.svelte';
  import { labelLocation, searchTowns, type TownMatch } from './places';
  import type { Community, Profile, Area } from './community';
  let {
    client,
    profile,
    refresh,
  }: { client: Community; profile: Profile; refresh: () => Promise<void> } = $props();
  let directory = $state.raw<ReturnType<typeof searchTowns>>(),
    results = $state.raw<TownMatch[]>([]),
    searching = $state(false);
  let areaSummary: HTMLParagraphElement;
  function dismiss() {
    generation++;
    directory?.close();
    directory = undefined;
  }
  $effect(() => {
    if (directory) {
      searching = query.trim().length >= 2;
      results = [];
      directory.search(query);
    }
  });
  let query = $state(''),
    center = $state<Area | null>(null),
    radius = $state(25),
    busy = $state(false),
    error = $state(''),
    locating = $state(false);
  let generation = 0;
  function prepare() {
    generation++;
    locating = false;
    center = profile.nearby?.center || null;
    radius = profile.nearby?.radiusMiles || 25;
    query = '';
    error = '';
    directory = searchTowns((matches, failed) => {
      results = matches;
      searching = false;
      if (failed) error = 'Couldn’t load towns. Reopen to try again.';
    });
  }
  function cancelLocation() {
    generation++;
    locating = false;
    error = '';
  }
  function locate() {
    if (!navigator.geolocation) {
      error = 'Choose a town on this device.';
      return;
    }
    const attempt = ++generation;
    locating = true;
    error = '';
    navigator.geolocation.getCurrentPosition(
      async (position) => {
        if (attempt !== generation) return;
        const coordinates = {
          label: 'Current area',
          lat100: Math.round(position.coords.latitude * 100),
          lon100: Math.round(position.coords.longitude * 100),
        };
        const area = await labelLocation(coordinates).catch(()=>coordinates);
        if (attempt !== generation) return;
        center = area;
        locating = false;
        query = '';
      },
      () => {
        if (attempt === generation) {
          locating = false;
          error = 'Location unavailable. Choose a town or allow location in your browser.';
        }
      },
      { enableHighAccuracy: false, timeout: 10000 },
    );
  }
  async function apply(close: () => void) {
    busy = true;
    try {
      await client.setNearby(center ? { center, radiusMiles: radius } : null);
      close();
      await refresh();
    } catch {
      error = 'Couldn’t save your area. Try again.';
    } finally {
      busy = false;
    }
  }
</script>

<Sheet
  label={profile.nearby ? 'Change area' : 'Choose a town'}
  title="Posting area"
  triggerClass="place-heading"
  {prepare}
  {dismiss}
  {busy}
>
  {#snippet opener()}{profile.nearby ? 'Change area' : 'Choose a town'} <AppIcon name="chevronDown" />{/snippet}
  {#snippet children()}
    <label>
      Search for a city or town
      <input type="search" placeholder="City or town" bind:value={query} oninput={cancelLocation} disabled={busy} />
    </label>
    {#if query.trim().length >= 2}
      <div class="place-list">
        {#each results as town}
          <ActionButton onclick={async () => {
            cancelLocation();
            center = town.area;
            query = '';
            await tick();
            areaSummary.focus({ preventScroll: true });
          }}>{town.area.label}</ActionButton>
        {:else}
          <p role="status">{searching ? 'Searching towns…' : 'No matching towns. Try another spelling.'}</p>
        {/each}
      </div>
    {/if}
    <div class="area-alternatives">
      <ActionButton disabled={busy || locating} onclick={locate}>{locating ? 'Finding location…' : 'Use my location'}</ActionButton>
      <ActionButton disabled={busy} onclick={() => {
        cancelLocation();
        center = null;
        query = '';
      }}>All cities</ActionButton>
    </div>
    <div class="area-selection">
      <p aria-live="polite" tabindex="-1" bind:this={areaSummary}>{center?.label || 'All cities'}</p>
      {#if center}
        <label>Feed radius (miles)<input type="number" min="1" max="250" step="1" bind:value={radius} disabled={busy} /></label>
        <p class="hint">New posts share this approximate area. The radius only changes which posts you see.</p>
      {:else}
        <p class="hint">See posts from every town on your home. New posts won’t include a location.</p>
      {/if}
    </div>
    {#if error}<p role="alert">{error}</p>{/if}
    <p class="hint town-credit">
      Town data from <a href="https://www.geonames.org/">GeoNames</a> ·
      <a href="https://creativecommons.org/licenses/by/4.0/">CC BY 4.0</a>.
    </p>
  {/snippet}
  {#snippet actions(close)}
    <ActionButton
      variant="filled"
      disabled={busy || locating || !!query.trim() || (!!center && !(Number.isInteger(radius) && radius >= 1 && radius <= 250))}
      onclick={() => apply(close)}>Apply area</ActionButton>
  {/snippet}
</Sheet>

<style>
  .area-alternatives { display: flex; flex-wrap: wrap; gap: .25rem .625rem; }
  .area-selection { border-block-start: 1px solid var(--line); padding-block-start: 1rem; }
  .area-selection > p:first-child { margin: 0 0 .75rem; font: var(--type-title); }
  .area-selection label { max-width: 13rem; }
  .town-credit { margin-block-end: 0; }
  .place-list {
    scrollbar-width: thin;
    scrollbar-color: var(--outline) transparent;
  }
</style>
