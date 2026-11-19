<script lang="ts">
  import { Menu, Portal } from '@skeletonlabs/skeleton-svelte';
  import AppIcon from './AppIcon.svelte';
  let { label, items }: { label: string; items: { label: string; run: () => void }[] } = $props();
  function select(value: string) {
    const item = items[Number(value)];
    if (item) setTimeout(item.run, 0);
  }
</script>

<Menu onSelect={({ value }) => select(value)} positioning={{ placement: 'bottom-end', gutter: 4, overflowPadding: 8 }}>
  <Menu.Trigger class="btn-icon post-menu-button" aria-label={label} title={label}>
    <AppIcon name="more" />
  </Menu.Trigger>
  <Menu.Context>
    {#snippet children(menu)}
      {#if menu().open}
        <Portal>
          <Menu.Positioner>
            <Menu.Content class="action-menu" aria-label={label}>
              {#each items as item, index (item.label)}
                <Menu.Item class="action-item" value={String(index)} valueText={item.label}>
                  {item.label}
                </Menu.Item>
              {/each}
            </Menu.Content>
          </Menu.Positioner>
        </Portal>
      {/if}
    {/snippet}
  </Menu.Context>
</Menu>

<style>
  :global(.post-menu-button) {
    width: 44px;
    height: 44px;
    display: inline-grid;
    place-items: center;
    padding: 0;
    border: 0;
    border-radius: .5rem;
    color: var(--quiet);
    background: transparent;
  }
</style>
