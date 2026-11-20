<script lang="ts">
  import ActionButton from './ActionButton.svelte';
  import { MAX_ATTACHMENT_BYTES } from './community';

  let { file = $bindable(null), disabled = false }: { file?: File | null; disabled?: boolean } = $props();
  let input: HTMLInputElement;
  let error = $state('');

  function select(event: Event) {
    const picker = event.currentTarget as HTMLInputElement;
    const selected = picker.files?.[0];
    picker.value = '';
    if (!selected) return;
    error = '';
    if (selected.size < 1 || selected.size > MAX_ATTACHMENT_BYTES) {
      error = 'Choose a file within 20 MB.';
      return;
    }
    file = selected;
  }
</script>

<div class="attachment-picker">
  <input type="file" hidden bind:this={input} onchange={select} {disabled} />
  {#if file}
    <span>{file.name}</span>
    <ActionButton type="button" disabled={disabled} onclick={() => { file = null; error = ''; }}>Remove file</ActionButton>
  {:else}
    <ActionButton type="button" disabled={disabled} onclick={() => input.click()}>Attach file</ActionButton>
  {/if}
  {#if error}<p role="alert">{error}</p>{/if}
</div>

<style>
  .attachment-picker { display: flex; flex-wrap: wrap; align-items: center; gap: .5rem; min-width: 0; }
  span { overflow-wrap: anywhere; }
  p { margin: 0; color: var(--error); }
</style>
