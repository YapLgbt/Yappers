<script lang="ts">
  import type { HTMLButtonAttributes } from 'svelte/elements';

  // Keep the browser's submitter, dialog return value, validation and focus behavior.
  let { variant = 'text', children, ...attributes }: HTMLButtonAttributes & {
    variant?: 'filled' | 'tonal' | 'text' | 'outlined';
  } = $props();
</script>

<button {...attributes} class={['mtrl-button', 'mtrl-button--' + variant, attributes.class]}>
  {@render children?.()}
</button>
