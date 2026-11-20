<script lang="ts">
  import AppIcon from './AppIcon.svelte';
  import { PROFILE_PHOTO_LENGTH, PROFILE_PHOTO_SIZE } from './community';

  let {
    photo,
    label,
    large = false,
    portrait = false,
  }: { photo?: string; label: string; large?: boolean; portrait?: boolean } = $props();
  let canvas = $state<HTMLCanvasElement>(),
    renderedPhoto = $state<string>();
  $effect(() => {
    const target = canvas,
      encoded = photo;
    let cancelled = false;
    renderedPhoto = undefined;
    if (target && encoded && encoded.length <= PROFILE_PHOTO_LENGTH) {
      void (async () => {
        const binary = Uint8Array.from(atob(encoded), (c) => c.charCodeAt(0));
        const img = await createImageBitmap(new Blob([binary]));
        try {
          const context = target.getContext('2d');
          if (!cancelled && context && Math.max(img.width, img.height) <= PROFILE_PHOTO_SIZE) {
            const size = Math.min(img.width, img.height);
            target.width = target.height = size;
            context.drawImage(img, (img.width - size) / 2, (img.height - size) / 2, size, size, 0, 0, size, size);
            renderedPhoto = encoded;
          }
        } finally {
          img.close();
        }
      })().catch(() => {});
    }
    return () => {
      cancelled = true;
    };
  });
</script>

<span class="avatar" class:large class:portrait class:has-photo={!!photo && renderedPhoto === photo}
  aria-hidden={portrait ? undefined : true} role={portrait ? 'img' : undefined}
  aria-label={portrait ? `Profile photo of ${label}` : undefined}>
  {#if !portrait && photo === ''}
    <img src="/app/default-avatar.png" alt="" />
  {:else if !portrait && (!photo || renderedPhoto !== photo)}
    <AppIcon name="you" />
  {/if}
  <canvas hidden={!photo || renderedPhoto !== photo} bind:this={canvas}></canvas>
</span>

<style>
  img { width: 100%; height: 100%; border-radius: inherit }
  .portrait { display: block; width: 100%; height: auto; aspect-ratio: 1; border-radius: var(--corner-large); overflow: hidden; }
  .portrait:not(.has-photo) { display: none; }
</style>
