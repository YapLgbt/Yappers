<script lang="ts">
  import AppIcon from './AppIcon.svelte';

  let {
    photo,
    large = false,
  }: { photo?: string; label: string; large?: boolean } = $props();
  let canvas = $state<HTMLCanvasElement>(),
    renderedPhoto = $state<string>();
  $effect(() => {
    const target = canvas,
      encoded = photo;
    let cancelled = false;
    renderedPhoto = undefined;
    if (target && encoded && encoded.length <= 4096) {
      void (async () => {
        const binary = Uint8Array.from(atob(encoded), (c) => c.charCodeAt(0));
        const img = await createImageBitmap(new Blob([binary]));
        try {
          const context = target.getContext('2d');
          if (!cancelled && context) {
            context.clearRect(0, 0, 96, 96);
            context.drawImage(img, 0, 0, 96, 96);
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

<span class="avatar" class:large aria-hidden="true">
  {#if photo === ''}
    <img src="/app/default-avatar.png" alt="" />
  {:else if !photo || renderedPhoto !== photo}
    <AppIcon name="you" />
  {/if}
  <canvas hidden={!photo || renderedPhoto !== photo} width="96" height="96" bind:this={canvas}></canvas>
</span>

<style>
  img { width: 100%; height: 100%; border-radius: inherit }
</style>
