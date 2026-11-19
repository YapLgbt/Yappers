<script lang="ts">
  import { DropdownMenu } from 'bits-ui';
  import AppIcon from './AppIcon.svelte';
  let { label, items }: { label: string; items: { label: string; run: () => void }[] } = $props();
</script>

<DropdownMenu.Root>
  <DropdownMenu.Trigger class="btn-icon post-menu-button" aria-label={label} title={label}
    ><AppIcon name="more" /></DropdownMenu.Trigger
  >
  <DropdownMenu.Portal>
    <DropdownMenu.Content class="action-menu" align="end" sideOffset={4} collisionPadding={8}>
      {#each items as item (item.label)}
        <DropdownMenu.Item
          class="action-item"
          onSelect={() => {
            setTimeout(item.run, 0);
          }}>{item.label}</DropdownMenu.Item
        >
      {/each}
    </DropdownMenu.Content>
  </DropdownMenu.Portal>
</DropdownMenu.Root>

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
  :global(.post-menu-button:focus-visible) { outline: 2px solid var(--plum); outline-offset: -2px }
  :global(.post-menu-button[aria-expanded=true]) { background: var(--wash); color: var(--plum) }
  @media (hover: hover) {
    :global(.post-menu-button:hover) { background: var(--wash); color: var(--plum) }
  }
</style>
