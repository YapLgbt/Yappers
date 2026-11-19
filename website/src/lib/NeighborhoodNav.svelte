<script lang="ts">
  import { Navigation } from '@skeletonlabs/skeleton-svelte';
  import { MediaQuery } from 'svelte/reactivity';
  import AppIcon from './AppIcon.svelte';

  const destinations = [
    { id: 'feed', icon: 'nearby', label: 'Nearby' },
    { id: 'groups', icon: 'groups', label: 'Groups' },
    { id: 'saved', icon: 'saved', label: 'Saved' },
    { id: 'you', icon: 'you', label: 'You' }
  ] as const;
  type Destination = typeof destinations[number]['id'];
  let { current, select }: { current: Destination | null; select: (id: Destination) => void } = $props();
  const desktop = new MediaQuery('(min-width: 900px)');
</script>

<Navigation layout={desktop.current ? 'sidebar' : 'bar'} role="navigation" aria-label="Neighborhood" class="app-nav">
  <Navigation.Menu>
    {#each destinations as item (item.id)}
      <Navigation.Trigger aria-current={current === item.id ? 'page' : undefined} onclick={() => select(item.id)}>
        <AppIcon name={item.icon} />
        <Navigation.TriggerText>{item.label}</Navigation.TriggerText>
      </Navigation.Trigger>
    {/each}
  </Navigation.Menu>
</Navigation>
