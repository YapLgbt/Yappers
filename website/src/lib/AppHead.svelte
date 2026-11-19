<script lang="ts">
  let { title = 'Neighborhood · Yap' }: { title?: string } = $props();
</script>

<svelte:head>
<meta name="color-scheme" content="light dark"><meta name="theme-color" content="#fcf8ff" media="(prefers-color-scheme:light)"><meta name="theme-color" content="#1b1522" media="(prefers-color-scheme:dark)">
<title>{title}</title><link rel="manifest" href="/app/manifest.webmanifest"><link rel="apple-touch-icon" href="/app/icon-192.png"><link rel="icon" href="/assets/icon.svg"><link rel="stylesheet" href="/app/app.css">
<link rel="stylesheet" href="/app/flag-print.css">

</svelte:head>
