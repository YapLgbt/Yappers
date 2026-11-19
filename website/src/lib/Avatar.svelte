<script module lang="ts">
  const initials = new Intl.Segmenter(undefined, { granularity: 'grapheme' });
</script>

<script lang="ts">
  let {
    photo = '',
    label,
    large = false,
  }: { photo?: string; label: string; large?: boolean } = $props();
  let canvas = $state<HTMLCanvasElement>(),
    loaded = $state(false);
  const initial = $derived(initials.segment(label.trim()).containing(0)?.segment.toLocaleUpperCase() || '');
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

<span class="avatar" class:large aria-hidden="true"
  >{#if !loaded}{initial}{/if}<canvas
    hidden={!loaded}
    width="96"
    height="96"
    bind:this={canvas}
  ></canvas></span
>
