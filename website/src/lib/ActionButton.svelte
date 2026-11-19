<script lang="ts">
  import type { HTMLButtonAttributes } from 'svelte/elements';

  // Keep the browser's submitter, dialog return value, validation and focus behavior.
  let { variant = 'text', children, ...attributes }: HTMLButtonAttributes & {
    variant?: 'filled' | 'tonal' | 'text' | 'outlined' | 'danger';
  } = $props();
</script>

<button {...attributes} class={['btn', attributes.class]} data-variant={variant}>
  {@render children?.()}
</button>
