<script lang="ts">
  import SiteFooter from "../lib/SiteFooter.svelte";
  import SiteHeader from "../lib/SiteHeader.svelte";
</script>
<svelte:head>
<meta name="color-scheme" content="light dark">
  <meta name="theme-color" media="(prefers-color-scheme: light)" content="#fcf8ff">
  <meta name="theme-color" media="(prefers-color-scheme: dark)" content="#1b1522">
  <title>Yap — Queer life, around here.</title>
  <meta name="description" content="Yap is a local conversation app for Queer folks. Find a reading buddy, plan a potluck, plan a date, or say hello to the person you keep seeing at the café.">
  <link rel="canonical" href="https://yap.lgbt/">
  <meta property="og:title" content="Yap — Queer life, around here.">
  <meta property="og:description" content="A local conversation app for Queer folks.">
  <meta property="og:image" content="https://yap.lgbt/app/estuary.webp">
  <meta property="og:image:width" content="660">
  <meta property="og:image:height" content="400">
  <meta property="og:image:alt" content="Rainbow Estuary with six colored bands following a tidal shoreline.">
  <link rel="icon" href="/assets/icon.svg?v=5" type="image/svg+xml">
  <link rel="preload" href="/assets/type/InstrumentSerif-Regular.ttf" as="font" type="font/ttf" crossorigin="anonymous">
  <link rel="preload" href="/assets/type/InstrumentSerif-Italic.ttf" as="font" type="font/ttf" crossorigin="anonymous">
  <link rel="stylesheet" href="/style.css">
<link rel="stylesheet" href="/app/flag-print.css">
<script type="module" src="/app/flag-print.js"></script>
</svelte:head>
<SiteHeader page="home" />
  <main id="main" tabindex="-1">
    <section class="hero wrap" aria-labelledby="welcome">
      <div class="intro">
        <h1 id="welcome">Queer life, <em>around here.</em></h1>
        <p>Yap is a local conversation app for Queer folks. Find a reading buddy, plan a potluck, plan a date, or say hello to the person you keep seeing at the café.</p>
        <a class="primary" href="https://discord.gg/GZMMvk2d3q">
          Join Discord for testing
          <svg class="link-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 12h15m-6-6 6 6-6 6" /></svg>
        </a>
      </div>
      <figure class="flag-print hero-art">
        <svg width="0" height="0" class="print-edge-defs" aria-hidden="true" focusable="false"><defs>
          <clipPath id="estuary-edge" clipPathUnits="objectBoundingBox">
            <path d="M.145 .154 C.256 .057 .420 .139 .531 .069 C.724 -.047 .939 .031 .928 .232 C.916 0.3790 .998 .383 .994 .548 C.985 .737 .822 .731 .760 .841 C.658 1.010 .486 1.032 .379 .936 C.289 0.8890 .085 .938 .047 .776 C.001 .593 .136 .551 .077 .407 C.015 0.2720 .051 .239 .145 .154Z"/>
          </clipPath></defs></svg>
        <div class="cloth">
          <img src="/app/estuary.webp" width="660" height="400" fetchpriority="high" alt="Rainbow Estuary, with shifting shorelines and six pride-colored bands.">
          <canvas hidden aria-hidden="true"></canvas>
        </div>
        <figcaption><span data-flag-name>Rainbow Estuary</span><button type="button" data-pause hidden>Pause motion</button></figcaption>
      </figure>
    </section>
    <section id="features" class="features" aria-labelledby="features-title">
      <div id="conversations" class="wrap feature-spread">
        <div class="feature-copy">
          <h2 id="features-title">Inside <em>Yap.</em></h2>
          <ul class="feature-list" role="list">
            <li><h3>Find your people</h3><p>Browse posts by topic, from weekend plans to missed connections.</p></li>
            <li><h3>Choose how you post</h3><p>Introduce yourself by name or ask a question anonymously. You get the same choice when you reply.</p></li>
            <li><h3>Send an encrypted DM</h3><p>On the web, iPhone, and Mac, you can send someone on your Home an end-to-end encrypted message. Only the two of you can read it.</p></li>
            <li><h3>Save plans for later</h3><p>Bookmark the potluck invite so you can find the time and place when you’re heading out.</p></li>
          </ul>
        </div>
        <div class="feed-preview">
          <img src="/assets/yap-demo-feed.webp" width="804" height="1748" loading="lazy" decoding="async" alt="Yap’s demo feed on iPhone, with a neighbor inviting readers to a quiet book club in the park.">
        </div>
      </div>
    </section>
    <section id="about" class="about wrap spread" aria-labelledby="about-title">
      <div class="section-intro"><h2 id="about-title">About <em>Yap.</em></h2><p>Yap is currently in testing.</p></div>
      <div class="answers">
        <details open>
          <summary>How local is my feed?<span class="toggle" aria-hidden="true"></span></summary>
          <p>Pick a city or town, or let Yap find your approximate location once. Choose a radius from 1 to 250 miles. Select All cities to browse the whole home feed.</p>
        </details>
        <details>
          <summary>What do people see when I post?<span class="toggle" aria-hidden="true"></span></summary>
          <p>Named posts include your community profile. Posting anonymously leaves those details out. A post in a local feed still shares your selected area, even when anonymous. Choose All cities to post without a location.</p>
          <p>The people who run your Home can still tell which account an anonymous post came from.</p>
        </details>
        <details>
          <summary>Federated and Open<span class="toggle" aria-hidden="true"></span></summary>
          <p>A Home is the community you join on Yap. You start at yap.lgbt, a Home for people in lots of different towns.</p>
          <p>Communities can run their own Homes. Each one chooses its rules and which other Homes it shares public conversations with. A queer book club could look after its own space and still share an open reading night with neighbors on other Homes.</p>
          <p>The software that runs a Home is open source. Anyone can see how it works and use it to start a Home of their own. <a href="/source/">Explore the Home software</a>.</p>
        </details>
      </div>
    </section>
  </main>
  <SiteFooter top="#welcome" />
