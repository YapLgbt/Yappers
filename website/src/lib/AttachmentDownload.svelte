<script lang="ts">
  import ActionButton from './ActionButton.svelte';
  import type { Attachment, Community } from './community';

  let { attachment, client }: { attachment: Attachment; client: Community } = $props();
  let busy = $state(false), error = $state(false);
  async function download() {
    if (busy) return;
    busy = true;
    error = false;
    try { await client.downloadAttachment(attachment); }
    catch { error = true; }
    finally { busy = false; }
  }
</script>

<div class="attachment-download">
  <ActionButton type="button" disabled={busy} onclick={download}>{busy ? 'Downloading…' : 'Download ' + attachment.name}</ActionButton>
  {#if error}<p role="alert">Couldn’t download this attachment. Try again.</p>{/if}
</div>

<style>
  .attachment-download :global(.btn) { max-width: 100%; overflow-wrap: anywhere; }
  p { margin: .25rem 0 0; color: var(--error); }
</style>
