<script lang="ts">
  import SiteFooter from "../../lib/SiteFooter.svelte";
  import SiteHeader from "../../lib/SiteHeader.svelte";
</script>
<svelte:head>
<title>Peering covenant · Yap</title>
  <link rel="stylesheet" href="/style.css">
  <script defer src="/section-index.js"></script>
</svelte:head>
<SiteHeader page="peering" />
  <main id="main" class="legal-layout wrap" tabindex="-1">
    <header class="legal-heading"><h1>Peering covenant</h1><p>Version 1 · November 18, 2026</p></header>
    <nav class="section-index" aria-label="Covenant sections"><details open><summary>On this page</summary><ul><li><a href="#clause-1">Publish the running source</a></li><li><a href="#clause-2">Use content to provide the service</a></li><li><a href="#clause-3">Honor deletion and prevent replay</a></li><li><a href="#clause-4">Be accountable to peers</a></li></ul></details></nav>
    <article class="legal-text"><p><strong>External peering is closed.</strong> Only Yap-operated homes currently exchange federation records with the Yap home.</p>
    <p>This covenant is between Yap and the operator identified in the acceptance record. By accepting, I confirm that I am authorized to act for that operator and agree to these commitments for the identified home. Yap reviews each home before admitting it to specific communities.</p>
    <section><h2 id="clause-1">Publish the running source</h2><p>We will provide a public link, without login or payment, to the exact revision of our home implementation under an open-source license. We will include our modifications, build instructions, and custom components that copy, log, back up or delete federated content. We will update the link and our signed acceptance when we change the deployed revision. Credentials, signing keys and user data stay private.</p></section>
<section><h2 id="clause-2">Use content to provide the service</h2><p>We will disclose which providers process federated content and explain how our backups work. We take responsibility for copies held by those providers. We will use federated content to operate the service. We will neither sell it nor use it for advertising, model training or a separate archive of deleted content.</p></section>
<section><h2 id="clause-3">Honor deletion and prevent replay</h2><p>Once we receive a valid signed deletion, we will stop serving the payload, remove its plaintext and salt from storage under our control, and make them unrecoverable from managed backups. This includes attachments, thumbnails, logs, caches, indexes, replicas and raw snapshots. We will preserve the signed deletion, reject replay of the original payload, and apply known deletions before exposing a restored database. An offline home must catch up on deletions when it reconnects. If we discover a failure, we will promptly notify affected peers and remedy it. Our duties for content already received continue after peering ends.</p></section>
<section><h2 id="clause-4">Be accountable to peers</h2><p>We will identify the responsible operator and keep a working contact for peering and deletion incidents. We permit checks using disposable content, including signed creation, deletion and stale replay. Yap may decline peering or suspend new deliveries when a home fails its checks or breaches this covenant. A suspended operator must continue processing valid deletions within its former peering scope. An operator must accept a new covenant version before Yap grants admission under that version.</p></section>
    <section><h2>What users can verify</h2><p>A deletion check shows how a peer responded to a signed deletion. An independent operator can keep a private copy. Public posts can also be read and copied by anyone.</p>
    <p><a href="/peering/v1.txt">Download the covenant text operators sign</a> · <a href="/source/">View the running server source</a></p></section></article>
  </main>
  <SiteFooter top="#top" />
