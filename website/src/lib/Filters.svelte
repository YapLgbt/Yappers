<script lang="ts">
  import { tick } from 'svelte';
  import ActionButton from './ActionButton.svelte';
  import { topics } from './community';

  let { apply }: { apply: (sort: string, topic: string) => void } = $props();
  let dialog: HTMLDialogElement;
  let trigger: HTMLButtonElement;
  let sort = $state('new');
  let topic = $state('All');

  export async function open(currentSort: string, currentTopic: string, opener: HTMLButtonElement) {
    sort = currentSort;
    topic = currentTopic;
    trigger = opener;
    await tick();
    dialog.showModal();
    dialog.querySelector<HTMLElement>('#feed-sort input:checked')?.focus();
  }
</script>

<dialog id="feed-filters" bind:this={dialog} aria-labelledby="filter-title"
  onclose={() => trigger?.focus({ preventScroll: true })}>
  <form id="filter-form" onsubmit={event => { event.preventDefault(); apply(sort, topic); dialog.close(); }}>
    <h2 id="filter-title">Feed filters</h2>
    <fieldset id="feed-sort">
      <legend>Sort by</legend>
      <div class="btn-group">
        {#each ['new', 'popular'] as value (value)}
          <label class="sort-choice check">
            <input type="radio" name="feed-sort" {value} bind:group={sort} />
            <span>{value === 'new' ? 'New' : 'Popular'}</span>
          </label>
        {/each}
      </div>
    </fieldset>
    <fieldset id="feed-topic">
      <legend>Topic</legend>
      {#each ['All', ...topics] as value (value)}
        <label class="topic-choice check">
          {value === 'All' ? 'All topics' : value}
          <input type="radio" name="feed-topic" {value} bind:group={topic} />
        </label>
      {/each}
    </fieldset>
    <div class="actions">
      <ActionButton type="button" data-close onclick={() => dialog.close()}>Cancel</ActionButton>
      <ActionButton type="submit" variant="filled">Apply</ActionButton>
    </div>
  </form>
</dialog>

<style>
  #feed-filters { background: var(--bg) }
  #filter-form { display: grid; gap: 1.25rem }
  #feed-sort .btn-group {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: .25rem;
    padding: .25rem;
    border-radius: var(--radius-base);
    background: var(--wash);
  }
  .sort-choice {
    position: relative;
    display: grid;
    place-items: center;
    min-height: 44px;
    margin: 0;
    border-radius: var(--radius-base);
    color: var(--quiet);
    font: var(--type-label);
    cursor: pointer;
  }
  .sort-choice:has(:checked) {
    background: var(--bg);
    color: var(--plum);
    box-shadow: inset 0 0 0 1px var(--line);
  }
  .sort-choice:has(:focus-visible) { outline: 2px solid var(--plum); outline-offset: -2px }
  .sort-choice input { position: absolute; inset: 0; width: 100%; height: 100%; margin: 0; opacity: 0; cursor: pointer }
  fieldset { min-inline-size: 0; margin: 0; padding: 0; border: 0 }
  legend { margin-block-end: .375rem; padding: 0; font: var(--type-label); color: var(--quiet) }
  .topic-choice {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: .75rem;
    min-height: 44px;
    padding: .375rem .125rem;
    border-block-end: 1px solid var(--line);
    cursor: pointer;
  }
  .topic-choice input { width: 1.125rem; height: 1.125rem; margin: 0; accent-color: var(--plum) }
  .topic-choice:has(:checked) { font-weight: 600; color: var(--plum) }
  .topic-choice:has(:focus-visible) { outline: 2px solid var(--plum); outline-offset: -2px }
  .actions { justify-content: flex-end }
  @media (hover: hover) {
    .topic-choice:hover { background: var(--wash) }
  }
  @media (forced-colors: active) {
    .sort-choice:has(:checked) { border: 1px solid Highlight }
  }
</style>
