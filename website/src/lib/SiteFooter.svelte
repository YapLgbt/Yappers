<script lang="ts">
  let { top }: { top: string } = $props();
</script>
<footer class="wrap site-footer">
    <a class="wordmark" href="/" aria-label="Yap home"><img src="/assets/yap-wordmark.svg?v=3" width="1745" height="773" alt=""></a>
    <nav aria-label="Policies"><a href="/peering/">Peering covenant</a>
      <a href="/source/">Server source</a>
      <a href="/terms/">Terms of Service</a>
      <a href="/privacy-policy/">Privacy Policy</a></nav>
    <a class="back-top" href={top}>
      Back to top
      <svg class="link-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 20V5m-6 6 6-6 6 6" /></svg>
    </a>
  </footer>
