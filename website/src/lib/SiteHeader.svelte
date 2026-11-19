<script lang="ts">
  let { page }: { page: 'home' | 'terms' | 'privacy-policy' | 'peering' } = $props();
</script>

<a class="skip" href="#main">Skip to content</a>
<header class="masthead wrap" id={page === 'home' ? undefined : 'top'}>
  <a class="wordmark" href="/" aria-label="Yap home"><img src="/assets/yap-wordmark.svg?v=3" width="1745" height="773" alt=""></a>
  {#if page === 'home'}
    <nav aria-label="Main navigation">
      <a href="#features">Features</a>
      <a href="#about">About Yap</a>
    </nav>
  {:else}
    <nav aria-label="Policy navigation">
      <a href="/terms/" aria-current={page === 'terms' ? 'page' : undefined}>Terms of Service</a>
      <a href="/privacy-policy/" aria-current={page === 'privacy-policy' ? 'page' : undefined}>Privacy Policy</a>
    </nav>
  {/if}
</header>
