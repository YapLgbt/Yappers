<script lang="ts">
  import './skeleton.css';
  import 'material/styles/icon-button';
  import './material.css';
  import './fields.css';
  let { children } = $props();
</script>

<div data-theme="yap">{@render children()}</div>
