<script lang="ts">
  import ActionButton from './ActionButton.svelte';
  import type { Author } from './community';
  let { confirm }: { confirm: (target: Author) => void } = $props();
  let dialog: HTMLDialogElement, target: Author | null = null;
  export function open(author: Author) { target = author; dialog.returnValue = ''; dialog.showModal(); }
</script>
<dialog bind:this={dialog} onclose={() => { if (dialog.returnValue === "block" && target) confirm(target); target = null; }} id="block-dialog" aria-labelledby="block-title"><form method="dialog">
<h2 id="block-title">Block this account?</h2><p>When you sync, your home will hide this account’s posts and replies and stop messages between you.</p>
<div class="actions"><ActionButton value="cancel">Cancel</ActionButton><ActionButton value="block" variant="filled">Block account</ActionButton></div></form></dialog>
