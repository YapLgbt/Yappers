<script lang="ts">
  import { DropdownMenu } from 'bits-ui';
  import AppIcon from './AppIcon.svelte';
  let { label, items }: { label: string; items: { label: string; run: () => void }[] } = $props();
</script>

<DropdownMenu.Root>
  <DropdownMenu.Trigger class="icon-action mtrl-icon-button mtrl-icon-button--standard" aria-label={label}
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
