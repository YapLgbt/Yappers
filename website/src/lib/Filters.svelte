<script lang="ts">
  import { ButtonGroup, ButtonGroupItem, Chip, Chips } from 'material/svelte';
  import ActionButton from './ActionButton.svelte';
  import { topics } from './community';
  let { apply }: { apply: (sort: string, topic: string) => void } = $props();
  let dialog: HTMLDialogElement;
  let sort = $state('new'), topic = $state('All');
  export function open(currentSort: string, currentTopic: string) {
    sort = currentSort; topic = currentTopic; dialog.showModal();
  }
</script>
<dialog id="feed-filters" bind:this={dialog} aria-labelledby="filter-title">
  <form id="filter-form" onsubmit={event => { event.preventDefault(); apply(sort,topic); dialog.close(); }}>
    <h2 id="filter-title">Feed filters</h2>
    <div class="filter-sort">
      <h3>Sort by</h3>
      <ButtonGroup id="feed-sort" ariaLabel="Sort by" kind="connected" variant="tonal"
        selection="single" required equalWidth size="s" value={sort}
        onchange={({ detail }) => { if (typeof detail.value === 'string') sort = detail.value; }}>
        <ButtonGroupItem value="new">New</ButtonGroupItem>
        <ButtonGroupItem value="popular">Popular</ButtonGroupItem>
      </ButtonGroup>
    </div>
    <div class="filter-topic">
      <h3>Topic</h3>
      <Chips id="feed-topic" aria-label="Topic" selection="single" selectionRequired value={topic}
        onchange={({ detail }) => { if (typeof detail.value === 'string') topic = detail.value; }}>
        <Chip value="All" variant="filter">All topics</Chip>
        {#each topics as value (value)}
          <Chip {value} variant="filter">{value}</Chip>
        {/each}
      </Chips>
    </div>
    <div class="actions"><ActionButton type="button" data-close onclick={() => dialog.close()}>Cancel</ActionButton><ActionButton type="submit" variant="filled">Apply</ActionButton></div>
  </form>
</dialog>

<style>
  #filter-form {
    display: grid;
    gap: 1.25rem;
  }

  .filter-sort, .filter-topic {
    display: grid;
    gap: .5rem;
    justify-items: start;
  }

  .filter-sort :global(m-button-group) {
    inline-size: min(100%, 20rem);
  }

  .filter-sort :global(m-button-group)::part(button-group) {
    inline-size: 100%;
  }

  h3 {
    margin: 0;
    font: var(--type-label);
  }

  .actions {
    justify-content: flex-end;
  }
</style>
