<script lang="ts">
  import SiteFooter from "../../lib/SiteFooter.svelte";
  import SiteHeader from "../../lib/SiteHeader.svelte";
</script>
<svelte:head>
<title>Privacy Policy · Yap</title>
  <link rel="stylesheet" href="/style.css">
<script defer src="/section-index.js"></script>
</svelte:head>
<SiteHeader page="privacy-policy" />
  <main id="main" class="legal-layout wrap" tabindex="-1">
    <header class="legal-heading">
      <h1>Privacy Policy</h1>
      <p><time datetime="2026-11-18">Updated November 18, 2026</time> <span class="policy-age">For people 18 and older</span></p>
    </header>
    <nav class="section-index" aria-label="Sections on this page">
      <details open>
        <summary>On this page</summary>
        <ul>
          <li><a href="#when-you-visit-or-join">When you visit or join</a></li>
          <li><a href="#what-travels-with-a-post">What travels with a post</a></li>
          <li><a href="#nearby-and-groups">Nearby and groups</a></li>
          <li><a href="#direct-messages-and-safety-settings">Direct messages and safety settings</a></li>
          <li><a href="#what-stays-with-you">What stays with you</a></li>
          <li><a href="#who-else-can-hold-a-copy">Who else can hold a copy</a></li>
          <li><a href="#retention-deletion-and-your-choices">Retention, deletion and your choices</a></li>
        </ul>
      </details>
    </nav>

    <article class="legal-text" aria-label="Privacy Policy">
      <p>Yap operates yap.lgbt, the Yap apps and the default community server, which we call your “home.” We use information to deliver the service you choose, keep it working and protect the people using it. We do not sell personal data, target advertising or track you across other websites.</p>
      <section>
        <h2 id="when-you-visit-or-join">When you visit or join</h2>
    <p>Visiting the website sends ordinary connection information, including your IP address and the page you request, to our hosting infrastructure. When the app connects, your home and any network relay involved can see connection information such as network addresses and timing. We use it to route requests, diagnose failures and protect the service.</p>
    <p>You can join the default home without giving us your legal name, email address or phone number. Your device creates keys for your account. Your home receives public account and device identifiers and signed proof that your device can act for you. These let it recognize you across visits and verify what you publish. If you claim a username, your home records it against your account so others can recognize your named posts and replies.</p>
      </section>
      <section>
        <h2 id="what-travels-with-a-post">What travels with a post</h2>
    <p>Your home receives the posts, replies and votes you submit, along with their signatures and the community they belong to. It exchanges those records with other homes that host that feed. Anyone can join the public yap.lgbt feed. Private communities limit participation, but their operators and authorized peers can still read and store feed content.</p>
    <p>Named posts carry a snapshot of the profile you shared at the time. It can include your display name, username, photo thumbnail, pronouns, identity, interests and bio. These optional details may reveal your gender identity or sexual orientation. You choose what to include. Changing your profile changes future posts. Older posts keep their original snapshot.</p>
    <p><strong>Posting anonymously hides your byline and profile from the feed.</strong> The signed record still lets your home and receiving operators connect that post to your account and your other activity. Readers may also recognize details in what you write.</p>
      </section>
      <section>
        <h2 id="nearby-and-groups">Nearby and groups</h2>
    <p>You can choose a town, allow a one-time location fix or use All cities. The web app searches a town directory on your device. On iPhone and Mac, Apple’s map services may receive your town search or a rounded location to look up a place name. For nearby feeds, your home receives your selected center rounded to hundredths of a degree and your search radius. New home-feed posts include the area label and rounded coordinates, even if you post anonymously. These can identify a small area. Choose <strong>All cities</strong> before posting to leave location off new posts.</p>
    <p>Your home handles group creation and join requests. Public group names, descriptions of their area and information about which homes host them can be shared through the group directory.</p>
      </section>
      <section>
        <h2 id="direct-messages-and-safety-settings">Direct messages and safety settings</h2>
    <p>Direct messages in Yap are end-to-end encrypted between devices on the same home. The home handles encrypted packets and can see the participants, times, sizes and volume of messages. It holds queued packets for up to 30 days. Message text is readable on the participants’ devices, and a recipient can copy or share it.</p>
    <p>Your home processes blocks and message-delivery preferences to filter your feed and mailbox. Hiding a post or muting a thread changes your own view.</p>
    <p>On homes that accept safety reports, you can review and send a reason, text you select and an optional post, reply or account ID to your primary home's moderator. If you select text from an encrypted message, that excerpt becomes readable to the moderator. A report is a separate copy and may survive deletion of the original. Yap’s home software keeps it out of federation and feed backups. The inbox removes evidence when the moderator closes the case or after seven days, and removes the case status after 30 days. Expiry runs while the inbox is online and when it reopens. Your private receipt lets its holder check the outcome. Details you submit can identify you. An independent operator can copy evidence or use software with different retention rules.</p>
      </section>
      <section>
        <h2 id="what-stays-with-you">What stays with you</h2>
    <p>Your device keeps account secrets, saved posts, cached feeds and pending uploads. The web app uses browser storage and an offline cache so you can reopen your feed. Message history stays on the messaging device and is excluded from portable account backups. Recovery runs on your device. If you save a key in a password manager or export a recovery file, you choose where to keep it.</p>
    <p>Before clearing site storage or removing the app, save the recovery material you need. Clearing browser storage removes that browser’s local account and unsent work. It has no effect on posts already delivered.</p>
      </section>
      <section>
        <h2 id="who-else-can-hold-a-copy">Who else can hold a copy</h2>
    <p>We use hosting infrastructure, including Fly.io, and network relays to serve the website, connect devices and keep the home available. An authorized standby can hold home records, including account state, blocks and encrypted mail. A peer that only replicates a public feed receives feed records. We may also disclose information we hold when legally required.</p>
    <p>Independent homes can be in other countries and have their own operators, policies and retention practices. Their users can receive your posts even if you never visit their home. This policy covers Yap’s handling of data, including records we receive from those homes.</p>
      </section>
      <section>
        <h2 id="retention-deletion-and-your-choices">Retention, deletion and your choices</h2>
    <p>Aside from feature-specific limits such as queued mail, Yap has no fixed schedule for deleting service records, operational logs or backups. On homes using Yap’s version 5 protocol, deleting your post or reply erases its text, shared profile photo and location when the home processes your deletion. The home keeps signed evidence of who posted and who deleted it. Yap’s version 5 backup system also discards the key needed to read that item in older backups. A home that is offline can only apply the deletion when it reconnects. Earlier versions keep the original signed content. Other operators, readers and people with their own recovery files may keep copies indefinitely.</p>
    <p>You can edit your profile, change your location and privacy preferences, or stop using the service. On iPhone and Mac, open <strong>You → Your data</strong> to back up your account and authored history or move to a compatible home. On the web, <strong>You → Account data</strong> supports encrypted backups and moving homes. <strong>Preferences → Back up account</strong> saves a key for recovering access. Moving homes leaves earlier shared copies in place.</p>
    <p>Depending on where you live, you may have rights to access, correct, erase or obtain a copy of your data, restrict processing, object to it or complain to a data protection authority. Yap has not yet designated a public privacy contact. For records under an independent home’s control, consult that operator’s policy.</p>
    <p>We will post revisions on this page with an updated date.</p>
      </section>
    </article>
  </main>
  <SiteFooter top="#top" />
