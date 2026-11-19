<script lang="ts">
  import { Switch } from 'material/svelte';
  let { id, label, hint, checked, change }: {
    id:string; label:string; hint?:string; checked:boolean; change:(next:boolean)=>void;
  } = $props();
</script>

<div class="privacy-switch">
  <label for={id}>{label}</label>
  <Switch {id} ariaLabel={label} {checked}
    onchange={(event: CustomEvent<{checked:boolean}>) => change(event.detail.checked)}/>
  {#if hint}<p class="hint">{hint}</p>{/if}
</div>
