<script lang="ts">
  import type { Snippet } from 'svelte';
  let {
    label,
    title,
    children,
    actions,
    prepare,
    dismiss,
    canDismiss,
    triggerClass,
    triggerVariant = 'tonal',
    busy = false,
    disabled = false,
    opener,
  }: {
    label: string;
    title: string;
    children: Snippet<[() => void]>;
    actions?: Snippet<[() => void]>;
    prepare?: () => void;
    dismiss?: () => void;
    canDismiss?: () => boolean;
    triggerClass?: string;
    triggerVariant?: 'tonal' | 'text';
    busy?: boolean;
    disabled?: boolean;
    opener?: Snippet;
  } = $props();
  const titleId = $props.id();
  let open = $state(false);
  let dialog = $state<HTMLDialogElement>();
  let trigger: HTMLButtonElement;

  function present(element: HTMLDialogElement) { element.showModal(); }

  function show() {
    prepare?.();
    trigger.focus({ preventScroll: true });
    open = true;
  }

  function close() {
    dialog?.close();
    open = false;
    dismiss?.();
    trigger.focus({ preventScroll: true });
  }
</script>

<button type="button" bind:this={trigger} class={triggerClass ?? 'btn'}
  data-variant={triggerClass ? undefined : triggerVariant} {disabled} aria-label={label}
  aria-haspopup="dialog" onclick={show}>
  {#if opener}{@render opener()}{:else}{label}{/if}
</button>
{#if open}
  <dialog bind:this={dialog} use:present class="app-sheet" closedby="any" aria-labelledby={titleId}
    oncancel={event => {
      event.preventDefault();
      if (!busy && canDismiss?.() !== false) close();
    }}>
    <div class="dialog-title">
      <h2 id={titleId}>{title}</h2>
      <button type="button" class="btn" data-variant="text" disabled={busy}
        onclick={() => { if (canDismiss?.() !== false) close(); }}>Close</button>
    </div>
    <div class="app-sheet-body">{@render children(close)}</div>
    {#if actions}<div class="app-sheet-actions">{@render actions(close)}</div>{/if}
  </dialog>
{/if}
