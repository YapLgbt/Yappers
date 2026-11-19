<script lang="ts">
  import { Dialog } from 'bits-ui';
  import type { Snippet } from 'svelte';
  let {
    label,
    title,
    children,
    actions,
    prepare,
    dismiss,
    canDismiss,
    triggerClass = 'mtrl-button mtrl-button--tonal',
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
    busy?: boolean;
    disabled?: boolean;
    opener?: Snippet;
  } = $props();
  let open = $state(false),
    trigger = $state<HTMLButtonElement | null>(null);
  function change(next: boolean) {
    if (!next && (busy || canDismiss?.() === false)) return;
    if (next) prepare?.();
    else dismiss?.();
    open = next;
  }

  function close() {
    dismiss?.();
    open = false;
  }
</script>

<Dialog.Root bind:open={() => open, change}>
  <Dialog.Trigger bind:ref={trigger} class={triggerClass} {disabled} aria-label={label}
    >{#if opener}{@render opener()}{:else}{label}{/if}</Dialog.Trigger
  >
  <Dialog.Portal>
    <Dialog.Overlay class="sheet-backdrop" />
    <Dialog.Content
      onCloseAutoFocus={(event) => {
        event.preventDefault();
        trigger?.focus({ preventScroll: true });
      }}
      class="app-sheet"
      escapeKeydownBehavior={busy ? 'ignore' : 'close'}
      interactOutsideBehavior={busy ? 'ignore' : 'close'}
    >
      <div class="dialog-title">
        <Dialog.Title>{title}</Dialog.Title><Dialog.Close class="mtrl-button mtrl-button--text" disabled={busy}
          >Close</Dialog.Close
        >
      </div>
      <div class="app-sheet-body">
        {@render children(close)}
      </div>
      {#if actions}<div class="app-sheet-actions">{@render actions(close)}</div>{/if}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
