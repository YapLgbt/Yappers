<script lang="ts">
  import { Switch } from '@skeletonlabs/skeleton-svelte';
  let { id, label, hint, checked, change }: {
    id:string; label:string; hint?:string; checked:boolean; change:(next:boolean)=>void;
  } = $props();
</script>

<div>
  <Switch class="privacy-switch" ids={{ hiddenInput: id }} {checked}
    onCheckedChange={({ checked }) => change(checked)}>
    <Switch.Label>{label}</Switch.Label>
    <Switch.Control><Switch.Thumb /></Switch.Control>
    <Switch.HiddenInput role="switch" aria-describedby={hint ? id + '-hint' : undefined} />
  </Switch>
  {#if hint}<p class="hint" id={id + '-hint'}>{hint}</p>{/if}
</div>
