<script lang="ts">
  let {
    photo = '',
    large = false,
  }: { photo?: string; label: string; large?: boolean } = $props();
  let canvas = $state<HTMLCanvasElement>(),
    loaded = $state(false);
  $effect(() => {
    const target = canvas,
      encoded = photo;
    let cancelled = false;
    loaded = false;
    if (target && encoded && encoded.length <= 4096) {
      void (async () => {
        const binary = Uint8Array.from(atob(encoded), (c) => c.charCodeAt(0));
        const img = await createImageBitmap(new Blob([binary]));
        try {
          if (!cancelled) {
            target.getContext('2d')?.drawImage(img, 0, 0, 96, 96);
            loaded = true;
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
  {#if !loaded}<img src="/app/default-avatar.png" alt="" />{/if}
  <canvas hidden={!loaded} width="96" height="96" bind:this={canvas}></canvas>
</span>

<style>
  img { width: 100%; height: 100%; border-radius: inherit }
</style>
