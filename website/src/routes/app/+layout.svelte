<script lang="ts">
  import './skeleton.css';
  import './profile.css';
  import './fields.css';
  let { children } = $props();
</script>

<div data-theme="yap">{@render children()}</div>
