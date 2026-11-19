<script lang="ts">
  import { MediaQuery } from 'svelte/reactivity';
  import { NavigationBar, NavigationBarItem, NavigationRail, NavigationRailItem } from 'material/svelte';
  import { paths } from './iconPaths';

  const destinations = [
    { id: 'feed', icon: 'nearby', label: 'Nearby' },
    { id: 'groups', icon: 'groups', label: 'Groups' },
    { id: 'saved', icon: 'saved', label: 'Saved' },
    { id: 'you', icon: 'you', label: 'You' }
  ] as const;
  type Destination = typeof destinations[number]['id'];
  let { current, select }: { current: Destination | null; select: (id: Destination) => void } = $props();

  const wide = new MediaQuery('(min-width: 900px)');
  let Navigation = $derived(wide.current ? NavigationRail : NavigationBar);
  let Item = $derived(wide.current ? NavigationRailItem : NavigationBarItem);

  function change(event: CustomEvent<{value: string}>) {
    const item = destinations.find(({id}) => id === event.detail.value);
    if (item) select(item.id);
  }
</script>

<div class="app-nav">
  <Navigation aria-label="Neighborhood" value={current} onchange={change}
    {...(wide.current ? { noToggle:true, expanded:true, expandedWidth:208 } : { itemLayout:'vertical' })}>
    {#each destinations as item (item.id)}
      <Item value={item.id} label={item.label}
        icon={'<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path fill="none" d="'+paths[item.icon]+'"/></svg>'}/>
    {/each}
  </Navigation>
</div>
