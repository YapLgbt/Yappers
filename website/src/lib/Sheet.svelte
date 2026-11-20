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
    appearance,
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
    appearance?: 'profile';
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
  <dialog bind:this={dialog} use:present class="app-sheet" class:profile-sheet={appearance === 'profile'}
    closedby="any" aria-labelledby={appearance === 'profile' ? undefined : titleId}
    aria-label={appearance === 'profile' ? title : undefined}
    oncancel={event => {
      event.preventDefault();
      if (!busy && canDismiss?.() !== false) close();
    }}>
    <div class="dialog-title">
      {#if appearance !== 'profile'}<h2 id={titleId}>{title}</h2>{/if}
      <button type="button" class="btn" data-variant="text" disabled={busy}
        onclick={() => { if (canDismiss?.() !== false) close(); }}>Close</button>
    </div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex (The profile may scroll without interactive children.) -->
    <div class="app-sheet-body" tabindex={appearance === 'profile' ? 0 : undefined}
      role={appearance === 'profile' ? 'region' : undefined}
      aria-label={appearance === 'profile' ? 'Profile details' : undefined}>
      {@render children(close)}
    </div>
    {#if actions}<div class="app-sheet-actions">{@render actions(close)}</div>{/if}
  </dialog>
{/if}
