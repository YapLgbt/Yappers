<script lang="ts">
  import { paths } from './iconPaths';
  let {name}: {name:keyof typeof paths} = $props();
</script>
<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width={name === 'more' ? 3 : 1.7} stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>
