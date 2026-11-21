<script lang="ts">
  import { onDestroy, onMount, tick } from 'svelte';
  import ActionButton from '../lib/ActionButton.svelte';
  import './admin.css';

  type Section = 'login' | 'storage' | 'peers' | 'invitation' | 'moderation' | 'hidden' | 'backup' | 'reports' | 'case';
  type Home = {
    id: string;
    storageOnline: boolean;
    networkMode: 'local' | 'n0';
    communities: string[];
    sharedFeed: string | null;
    reportsEnabled: boolean;
  };
  type Peer = { id: string; operator: string | null; status: 'active' | 'suspended' | 'blocked' | 'unapproved'; communities: string[] };
  const peerLabels = { active: 'Peering', suspended: 'Deletion exchange only', blocked: 'Blocked', unapproved: 'Awaiting admission' };
  type PendingReport = { case: string; receivedAt: number };
  type Report = PendingReport & {
    evidence: { category: string; subject: string; reference: string | null; text: string } | null;
  };
  type Content = { kind: 'post' | 'reply'; community: string; headline: string | null; body: string };
  type ModerationEvent = { hidden: boolean; content: Content | null };
  type HiddenPage = { ids: string[]; next: string | null };

  const categories: Record<string, string> = {
    harassment: 'Harassment', outing: 'Outing', stalking: 'Stalking',
    threat: 'Threat', intimateImage: 'Intimate image', other: 'Other',
  };
  const subjects: Record<string, string> = { post: 'Post', reply: 'Reply', account: 'Account', message: 'Message' };
  const dateFormat = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' });

  let token = $state<string | null>(null);
  let ready = $state(false);
  let sessionTimer: ReturnType<typeof setTimeout> | undefined;
  let code = $state('');
  let eventId = $state('');
  let lookup = $state<{ id: string; event: ModerationEvent } | null>(null);
  let hidden = $state<HiddenPage | null>(null);
  let eventTitle = $state<HTMLHeadingElement>();
  let hiddenTitle = $state<HTMLHeadingElement>();
  let outcome = $state('');
  let home = $state<Home | null>(null);
  let peers = $state<Peer[] | null>(null);
  let peerHeading = $state<HTMLHeadingElement>();
  let reports = $state<PendingReport[]>([]);
  let currentCase = $state<Report | null>(null);
  let reportedEvent = $state<ModerationEvent | null>(null);
  let reportReference = $derived.by(() => {
    const evidence = currentCase?.evidence;
    return evidence && (evidence.subject === 'post' || evidence.subject === 'reply') ? evidence.reference : null;
  });
  let reviewRequest = 0;
  let messages = $state<Partial<Record<Section, { text: string; error: boolean }>>>({});
  let busy = $state<Record<string, boolean>>({});
  let moderationBusy = $derived(busy.home || busy.moderation || busy.hidden || busy.reportEvent || busy.hideReport);
  let codeInput = $state<HTMLInputElement>();
  let homeTitle = $state<HTMLHeadingElement>();
  let caseTitle = $state<HTMLHeadingElement>();
  let reportedEventTitle = $state<HTMLHeadingElement>();
  let reportsTitle = $state<HTMLHeadingElement>();

  onMount(() => { ready = true; });
  onDestroy(() => clearTimeout(sessionTimer));

  function notice(section: Section, text = '', error = false) {
    messages[section] = { text, error };
  }

  function clearCase() {
    reviewRequest += 1;
    currentCase = null;
    reportedEvent = null;
    outcome = '';
    notice('case');
  }

  async function endSession(message = '') {
    clearTimeout(sessionTimer);
    token = null;
    home = null;
    peers = null;
    reports = [];
    code = '';
    eventId = '';
    lookup = null;
    hidden = null;
    clearCase();
    messages = {};
    if (message) notice('login', message, true);
    await tick();
    codeInput?.focus();
  }

  async function api<T>(path: string, body?: object): Promise<T> {
    const requestToken = token;
    const headers = new Headers({ Accept: 'application/json' });
    if (requestToken) headers.set('Authorization', `Bearer ${requestToken}`);
    if (body) headers.set('Content-Type', 'application/json');
    const response = await fetch(`/api/${path}`, {
      headers,
      method: body ? 'POST' : 'GET',
      body: body ? JSON.stringify(body) : undefined,
    });
    const data = response.status === 204 ? undefined : await response.json().catch(() => null);
    if (requestToken !== token) throw new Error('This session ended.');
    if (response.status === 401 && requestToken) {
      await endSession('Your session ended. Generate a new code to sign in.');
    }
    if (!response.ok) throw new Error(data?.error ?? 'The Home rejected the request. Try again.');
    return data as T;
  }

  async function perform(key: string, section: Section, operation: () => Promise<void>) {
    if (busy[key]) return;
    const requestToken = token;
    busy[key] = true;
    try {
      await operation();
    } catch (error) {
      if (token === requestToken) notice(section, error instanceof Error ? error.message : 'Try again.', true);
    } finally {
      busy[key] = false;
    }
  }

  async function loadReports() {
    reports = await api<PendingReport[]>('reports');
    clearCase();
    notice('reports', reports.length ? '' : 'No reports awaiting review.');
  }

  function loadHome() {
    return perform('home', 'storage', async () => {
      notice('storage', 'Checking Home storage…');
      home = await api<Home>('home');
      notice('storage', home.storageOnline ? 'Storage online' : 'Storage offline. Start the Home to use maintenance actions.');
      if (home.storageOnline) await perform('peers', 'peers', loadPeers);
      else peers = null;
      lookup = null;
      notice('moderation');
      if (home.storageOnline) await refreshHidden();
      else {
        hidden = null;
        reportedEvent = null;
      }
      if (home.reportsEnabled) await perform('reports', 'reports', loadReports);
    });
  }

  async function loadPeers() {
    peers = await api<Peer[]>('peers');
    notice('peers', peers.length ? '' : 'This Home has no federation peers yet.');
  }

  function stopPeering(peer: Peer) {
    if (!window.confirm('Stop peering with this Home? Only eligible deletions will continue to sync.')) return;
    return perform('peers', 'peers', async () => {
      await api<void>(`peers/${peer.id}/stop`, {});
      // Update the returned state immediately, even if a later refresh fails.
      peers = peers?.map((current) => current.id === peer.id ? { ...current, status: 'suspended' } : current) ?? null;
      notice('peers', 'Peering stopped. Eligible deletions can still sync.');
      await tick();
      peerHeading?.focus();
    });
  }

  function signIn(event: SubmitEvent) {
    event.preventDefault();
    return perform('login', 'login', async () => {
      notice('login', 'Checking code…');
      const session = await api<{ token: string; sessionSeconds: number }>('login', { code });
      token = session.token;
      code = '';
      sessionTimer = setTimeout(() => endSession('Your session expired. Generate a new code to sign in.'), session.sessionSeconds * 1000);
      await tick();
      homeTitle?.focus();
      await loadHome();
    });
  }

  function signOut() {
    return perform('logout', 'storage', async () => {
      try { await api<void>('logout', {}); } finally { await endSession(); }
    });
  }

  function downloadInvitation(community: string, index: number) {
    return perform(`invite-${index}`, 'invitation', async () => {
      const invitation = await api<unknown>(`invitations/${index}`);
      const url = URL.createObjectURL(new Blob([JSON.stringify(invitation, null, 2)], { type: 'application/json' }));
      const download = document.createElement('a');
      download.href = url;
      download.download = `yap-invitation-${index}.json`;
      download.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
      notice('invitation', `Invitation downloaded for ${community.split('/').at(-1)}.`);
    });
  }

  function refreshHidden(after?: string) {
    return perform('hidden', 'hidden', async () => {
      notice('hidden', 'Loading hidden events…');
      const page = await api<HiddenPage>(after ? `moderation/hidden/${after}` : 'moderation');
      hidden = after && hidden ? { ...page, ids: [...hidden.ids, ...page.ids] } : page;
      notice('hidden');
    });
  }

  function inspectEvent(event: SubmitEvent) {
    event.preventDefault();
    return lookupEvent(eventId);
  }

  function lookupEvent(id: string) {
    if (!home?.storageOnline || moderationBusy) return;
    return perform('moderation', 'moderation', async () => {
      eventId = id;
      lookup = null;
      notice('moderation', 'Looking up event…');
      lookup = { id, event: await api<ModerationEvent>(`moderation/${id}`) };
      notice('moderation');
      await tick();
      eventTitle?.focus();
    });
  }

  function updateModeration(id: string, hide: boolean) {
    if (lookup?.id === id) lookup.event.hidden = hide;
    if (reportedEvent && reportReference === id) reportedEvent.hidden = hide;
    if (hidden && !hide) hidden.ids = hidden.ids.filter((current) => current !== id);
  }

  function moderate(id: string, hide: boolean, section: 'moderation' | 'hidden') {
    if (moderationBusy || !home?.storageOnline) return;
    return perform('moderation', section, async () => {
      notice(section, hide ? 'Hiding event…' : 'Restoring event…');
      await api<void>('moderation', { event: id, hide });
      updateModeration(id, hide);
      notice(section, hide ? 'Event hidden on this Home.' : 'Event restored on this Home.');
      if (hide) await refreshHidden();
      await tick();
      if (section === 'hidden') hiddenTitle?.focus();
      else eventTitle?.focus();
    });
  }

  function inspectReportedEvent() {
    if (!reportReference || moderationBusy || !home?.storageOnline) return;
    const id = reportReference;
    return perform('reportEvent', 'case', async () => {
      notice('case', 'Looking up the reported event…');
      reportedEvent = await api<ModerationEvent>(`moderation/${id}`);
      notice('case');
      await tick();
      reportedEventTitle?.focus();
    });
  }

  function hideReportedEvent() {
    const id = reportReference;
    const report = currentCase;
    if (!id || !report || moderationBusy || !home?.storageOnline) return;
    return perform('hideReport', 'case', async () => {
      notice('case', 'Hiding the reported event…');
      await api<void>(`reports/${report.case}/hide`, {});
      updateModeration(id, true);
      outcome = 'actionTaken';
      notice('case', 'Event hidden on this Home. The report is still open.');
      await refreshHidden();
      await tick();
      caseTitle?.focus();
    });
  }

  function createBackup() {
    return perform('backup', 'backup', async () => {
      notice('backup', 'Creating protected backup…');
      const { file } = await api<{ file: string }>('backups', {});
      notice('backup', `Saved on the Home as ${file}.`);
    });
  }

  function review(id: string) {
    if (busy[id] || busy.reportEvent || busy.hideReport) return;
    clearCase();
    const request = reviewRequest;
    return perform(id, 'reports', async () => {
      try {
        const report = await api<Report>(`reports/${id}`);
        if (request !== reviewRequest) return;
        if (!report.evidence) {
          notice('reports', 'This report was already closed or expired. Refresh the inbox.', true);
          return;
        }
        currentCase = report;
        await tick();
        caseTitle?.focus();
      } catch (error) {
        if (request === reviewRequest) throw error;
      }
    });
  }

  function closeCase(event: SubmitEvent) {
    event.preventDefault();
    const id = currentCase?.case;
    if (!id || busy.reports || busy.reportEvent || busy.hideReport || !window.confirm('Close this report and erase its submitted evidence?')) return;
    return perform('close', 'case', async () => {
      notice('case', 'Closing report…');
      try {
        await api<void>(`reports/${id}/close`, { outcome });
      } catch (error) {
        if (id === currentCase?.case) throw error;
        return;
      }
      clearCase();
      reports = reports.filter((report) => report.case !== id);
      notice('reports', 'Report closed and evidence erased.');
      reportsTitle?.focus();
    });
  }
</script>

<svelte:head>
  <title>Home admin · Yap</title>
</svelte:head>

{#snippet feedback(section: Section)}
  <p class="feedback" class:failure={messages[section]?.error} role="status">{messages[section]?.text ?? ''}</p>
{/snippet}

{#snippet eventContent(content: Content)}
  <p class="event-meta">{subjects[content.kind]} · <code>{content.community}</code></p>
  {#if content.headline}<p class="event-headline">{content.headline}</p>{/if}
  <p class="event-body">{content.body}</p>
{/snippet}

<div data-theme="yap" class="admin">
  <header class="topbar">
    <div class="wrap bar-content">
      <div class="brand">Yap <span>Home admin</span></div>
      {#if token}
        <ActionButton variant="outlined" type="button" disabled={busy.logout} onclick={signOut}>Sign out</ActionButton>
      {/if}
    </div>
  </header>

  <main class="wrap">
    {#if !token}
      <section class="sign-in" aria-labelledby="sign-in-title">
        <h1 id="sign-in-title">Sign in to your Home</h1>
        <p>Generate a one-time code for this Home, then paste it here. Codes expire after five minutes.</p>
        <form onsubmit={signIn}>
          <label for="login-code">One-time code</label>
          <input class="input" id="login-code" type="password" spellcheck="false" autocapitalize="off" autocomplete="off" required disabled={!ready} bind:this={codeInput} bind:value={code}>
          <ActionButton variant="filled" type="submit" disabled={!ready || busy.login}>Sign in</ActionButton>
          {@render feedback('login')}
        </form>
        <details class="code-help">
          <summary>How to get a code</summary>
          <p>Run this as the Home’s service user with the path to its config.</p>
          <pre><code>yappers-server admin code --config &lt;config-path&gt;</code></pre>
          <p>For Docker Compose, run this in the project directory.</p>
          <pre><code>docker compose exec admin yappers-server admin code --config /var/lib/yap/home/config.json</code></pre>
        </details>
      </section>
    {:else}
      <div class="page-title">
        <div>
          <h1 tabindex="-1" bind:this={homeTitle}>Manage your Home</h1>
          <p class="storage-status" class:online={home?.storageOnline && !busy.home} role="status">{messages.storage?.text ?? ''}</p>
        </div>
        <ActionButton variant="outlined" type="button" disabled={busy.home || busy.close || busy.peers || moderationBusy} onclick={loadHome}>Refresh</ActionButton>
      </div>
      {#if home}
        <dl class="home-facts">
          <div class="home-id"><dt>Home ID</dt><dd><code>{home.id}</code></dd></div>
          <div><dt>Network</dt><dd>{home.networkMode === 'n0' ? 'Iroh relays' : 'Local only'}</dd></div>
          <div><dt>Federation peers</dt><dd>{peers?.filter((peer) => peer.status === 'active').length ?? 'Unavailable'}</dd></div>
        </dl>

        <section class="section" aria-labelledby="invitations-title">
          <div class="section-intro">
            <h2 id="invitations-title">Invitations</h2>
            <p>Share invitations privately with people you want to join this Home.</p>
          </div>
          <div class="section-controls">
            <ul class="rows">
              {#each home.communities as community, index (community)}
                <li>
                  <div class="row-label">{community === home.sharedFeed ? 'Main Yap feed' : community.split('/').at(-1)}<code class="row-key">{community}</code></div>
                  <ActionButton variant="outlined" type="button" aria-label={`Download invitation for ${community}`}
                    disabled={!home.storageOnline || busy[`invite-${index}`]} onclick={() => downloadInvitation(community, index)}>Download JSON</ActionButton>
                </li>
              {/each}
            </ul>
            {@render feedback('invitation')}
          </div>
        </section>

        <section class="section" aria-labelledby="peers-title">
          <div class="section-intro">
            <h2 id="peers-title" tabindex="-1" bind:this={peerHeading}>Federation peers</h2>
            <p>Stopping a peer ends new content exchange with that Home. Eligible deletions can still sync.</p>
          </div>
          <div class="section-controls">
            {#if !home.storageOnline}
              <p class="feedback">Start the Home to view its current peers.</p>
            {:else if peers}
              <ul class="rows peer-list">
                {#each peers as peer (peer.id)}
                  <li>
                    <div class="row-label">
                      <strong>{peer.operator ?? 'Home'}</strong>
                      <code class="row-key">{peer.id}</code>
                      <p class="peer-status">{peerLabels[peer.status]}</p>
                      <details>
                        <summary>Federation scopes</summary>
                        <ul>
                          {#each peer.communities as scope (scope)}
                            <li>{scope === home?.sharedFeed ? 'Main Yap feed' : scope}</li>
                          {/each}
                        </ul>
                      </details>
                    </div>
                    {#if peer.status === 'active'}
                      <ActionButton variant="outlined" type="button" disabled={busy.peers}
                        aria-label={`Stop peering with ${peer.operator ?? peer.id}`} onclick={() => stopPeering(peer)}>Stop peering</ActionButton>
                    {/if}
                  </li>
                {/each}
              </ul>
            {:else if busy.peers}
              <p class="feedback" role="status">Loading peers…</p>
            {/if}
            {@render feedback('peers')}
          </div>
        </section>

        <section class="section" aria-labelledby="moderation-title">
          <div class="section-intro">
            <h2 id="moderation-title">Moderation</h2>
            <p>Hide content on this Home and restore it when needed. Each federated Home makes its own moderation decisions.</p>
          </div>
          <div class="section-controls moderation-tools">
            <form onsubmit={inspectEvent}>
              <label for="event-id">Event ID</label>
              <div class="event-search">
                <input class="input" id="event-id" spellcheck="false" autocapitalize="off" autocomplete="off" required maxlength="64" pattern={'[a-f0-9]{64}'}
                  readonly={busy.moderation} bind:value={eventId} oninput={() => { lookup = null; notice('moderation'); }}>
                <ActionButton variant="outlined" type="submit" disabled={!home.storageOnline || moderationBusy}>Look up event</ActionButton>
              </div>
            </form>
            {@render feedback('moderation')}
            {#if lookup}
              {@const { id, event } = lookup}
              <article class="event-preview" aria-labelledby="event-preview-title">
                <h3 id="event-preview-title" tabindex="-1" bind:this={eventTitle}>{event.hidden ? 'Hidden on this Home' : 'Event preview'}</h3>
                {#if event.content}
                  {@render eventContent(event.content)}
                {:else}
                  <p>No post or reply text is available for this ID. You can also hide an event before it arrives.</p>
                {/if}
                <ActionButton variant={event.hidden ? 'outlined' : 'filled'} type="button" disabled={!home.storageOnline || moderationBusy}
                  onclick={() => moderate(id, !event.hidden, 'moderation')}>{event.hidden ? 'Restore event' : 'Hide event'}</ActionButton>
              </article>
            {/if}
            <div class="hidden-events">
              <h3 tabindex="-1" bind:this={hiddenTitle}>Hidden events</h3>
              {#if !home.storageOnline}
                <p class="feedback">Start the Home to manage hidden events.</p>
              {:else if hidden}
                {#if hidden.ids.length}
                  <ul class="rows">
                    {#each hidden.ids as id (id)}
                      <li>
                        <code class="row-label row-key">{id}</code>
                        <div class="event-actions">
                          <ActionButton variant="outlined" type="button" disabled={moderationBusy}
                            aria-label={`View event ${id}`} onclick={() => lookupEvent(id)}>View</ActionButton>
                          <ActionButton variant="outlined" type="button" disabled={moderationBusy}
                            aria-label={`Restore event ${id}`} onclick={() => moderate(id, false, 'hidden')}>Restore</ActionButton>
                        </div>
                      </li>
                    {/each}
                  </ul>
                {:else if !busy.hidden && !hidden.next}
                  <p class="feedback">No hidden events.</p>
                {/if}
                {#if hidden.next}
                  {@const cursor = hidden.next}
                  <ActionButton variant="outlined" type="button" disabled={moderationBusy} onclick={() => refreshHidden(cursor)}>Load more hidden events</ActionButton>
                {/if}
              {/if}
              {@render feedback('hidden')}
            </div>
          </div>
        </section>

        <section class="section" aria-labelledby="reports-title">
          <div class="section-intro">
            <h2 id="reports-title" tabindex="-1" bind:this={reportsTitle}>Reports</h2>
            <p>Review submitted evidence and record an outcome. Closing a case erases its evidence.</p>
          </div>
          <div class="section-controls">
            {#if home.reportsEnabled}
              {@render feedback('reports')}
              <ul class="rows">
                {#each reports as report (report.case)}
                  <li>
                    <div class="row-label">{dateFormat.format(report.receivedAt * 1000)}<code class="row-key">{report.case}</code></div>
                    <ActionButton variant="outlined" type="button" disabled={busy[report.case] || busy.close || busy.reports || busy.reportEvent || busy.hideReport}
                      aria-label={`Review report ${report.case}`} onclick={() => review(report.case)}>Review report</ActionButton>
                  </li>
                {/each}
              </ul>
              {#if currentCase?.evidence}
                <article class="case-details" aria-labelledby="case-title">
                  <h3 id="case-title" tabindex="-1" bind:this={caseTitle}>Report details</h3>
                  <dl>
                    <div class="case-id"><dt>Case ID</dt><dd><code>{currentCase.case}</code></dd></div>
                    <div><dt>Category</dt><dd>{categories[currentCase.evidence.category]}</dd></div>
                    <div><dt>Subject</dt><dd>{subjects[currentCase.evidence.subject]}</dd></div>
                    {#if currentCase.evidence.reference}
                      <div><dt>Reference</dt><dd><code>{currentCase.evidence.reference}</code></dd></div>
                    {/if}
                  </dl>
                  <h4>Submitted evidence</h4>
                  <p class="evidence">{currentCase.evidence.text || 'The reporter submitted the reference only.'}</p>
                  {#if reportReference}
                    <div class="reported-event">
                      <h4 tabindex="-1" bind:this={reportedEventTitle}>Content on this Home</h4>
                      {#if reportedEvent}
                        {#if reportedEvent.content?.kind === currentCase.evidence.subject}
                          {@render eventContent(reportedEvent.content)}
                          {#if reportedEvent.hidden}
                            <p class="feedback">Already hidden on this Home.</p>
                          {:else}
                            <ActionButton variant="filled" type="button" disabled={!home.storageOnline || moderationBusy || busy.close}
                              onclick={hideReportedEvent}>Hide {subjects[currentCase.evidence.subject].toLowerCase()} on this Home</ActionButton>
                          {/if}
                        {:else}
                          <p class="feedback">No matching {subjects[currentCase.evidence.subject].toLowerCase()} is available on this Home.</p>
                        {/if}
                      {:else}
                        <ActionButton variant="outlined" type="button" disabled={!home.storageOnline || moderationBusy || busy.close}
                          onclick={inspectReportedEvent}>Look up reported event</ActionButton>
                      {/if}
                    </div>
                  {/if}
                  <form onsubmit={closeCase}>
                    <label for="case-outcome">Outcome</label>
                    <select class="select" id="case-outcome" required bind:value={outcome}>
                      <option value="">Choose an outcome</option>
                      <option value="actionTaken">Action taken</option>
                      <option value="noAction">No action</option>
                    </select>
                    <ActionButton variant="outlined" type="submit" disabled={busy.close || busy.reports || busy.reportEvent || busy.hideReport}>Close case and erase evidence</ActionButton>
                    {@render feedback('case')}
                  </form>
                </article>
              {/if}
            {:else}
              <p class="feedback">Reports are disabled. Set up a private report inbox to receive them.</p>
            {/if}
          </div>
        </section>

        <section class="section" aria-labelledby="backup-title">
          <div class="section-intro">
            <h2 id="backup-title">Backup</h2>
            <p>Save a protected feed archive in the Home’s private backups directory.</p>
          </div>
          <div class="section-controls">
            <ActionButton variant="outlined" type="button" disabled={!home.storageOnline || busy.backup} onclick={createBackup}>Create backup</ActionButton>
            {@render feedback('backup')}
          </div>
        </section>
      {/if}
    {/if}
  </main>
</div>
