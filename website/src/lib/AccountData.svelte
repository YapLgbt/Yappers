<script lang="ts">
  import { tick } from 'svelte';
  import ActionButton from './ActionButton.svelte';
  import Sheet from './Sheet.svelte';
  import { errorMessage, type Community, type Profile } from './community';
  let {
    client,
    profile,
    restoreOnly = false,
  }: { client: Community; profile: Profile | null; restoreOnly?: boolean } = $props();
  let mode = $state<'overview' | 'export' | 'import' | 'reset'>('overview');
  let phrase = $state(''),
    repeat = $state(''),
    invite = $state(''),
    file = $state<File>(),
    includeHome = $state(false),
    drafts = $state(false),
    confirmed = $state(false),
    busy = $state(false),
    error = $state(''),
    repeatError = $state(''),
    saved = $state('');
  const id = $props.id();
  let overview = $state<HTMLDivElement>();
  let form = $state<HTMLFormElement>();
  let confirmation = $state<HTMLInputElement>();
  function select(next: typeof mode) {
    mode = next;
    phrase = '';
    repeat = '';
    invite = '';
    file = undefined;
    includeHome = false;
    drafts = false;
    confirmed = false;
    error = '';
    repeatError = '';
    saved = '';
  }
  async function openMode(next: typeof mode) {
    select(next);
    await tick();
    (next === 'overview' ? overview?.querySelector('button') : form?.querySelector('input'))?.focus();
  }
  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (busy) return;
    error = '';
    if (mode === 'export' && phrase !== repeat) {
      repeatError = 'Enter the same passphrase again.';
      await tick();
      confirmation?.focus();
      return;
    }
    busy = true;
    try {
      if (mode === 'reset') {
        if (confirmed) await client.resetLocalData();
        return;
      }
      if (mode === 'import') {
        if (confirmed && file) await client.importArchive(file, phrase, invite, drafts);
        return;
      }
      const blob = await client.exportArchive(phrase, includeHome);
      const url = URL.createObjectURL(blob),
        link = document.createElement('a');
      link.href = url;
      link.download = 'Yap-Recovery.age';
      document.body.append(link);
      link.click();
      link.remove();
      setTimeout(() => URL.revokeObjectURL(url), 60000);
      await openMode('overview');
      saved = 'Backup prepared. Keep the file and its passphrase.';
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<Sheet
  triggerVariant="text"
  label={restoreOnly ? 'Restore a full backup' : 'Account data'}
  title={mode === 'export'
    ? 'Save a full backup'
    : mode === 'import'
      ? 'Restore a full backup'
      : mode === 'reset'
        ? 'Reset this browser?'
        : 'Account data'}
  {busy}
  actions={mode === 'overview' ? undefined : buttons}
  dismiss={() => select('overview')}
  prepare={() => select(restoreOnly || profile?.pendingTransfer ? 'import' : 'overview')}
>
  {#snippet children()}
    {#if mode === 'overview'}
      <div class="task-form account-overview" bind:this={overview}>
      {#if saved}<p class="form-feedback" role="status">{saved}</p>{/if}
      <p>Keep a copy of your profile and your posts, or bring them to another home.</p>
      <div class="form-actions">
        <ActionButton variant="tonal" onclick={() => openMode('export')}>Save a full backup</ActionButton>
        <ActionButton onclick={() => openMode('import')}>Restore a backup</ActionButton>
      </div>
      {#if profile?.recoveredDrafts?.length}<h3>Recovered drafts</h3>
        {#each profile.recoveredDrafts as draft (draft.id)}<p>
            {draft.payload.action.headline || ''} {draft.payload.action.body || draft.payload.action.kind}
          </p>{/each}{/if}
      {#if profile?.ownedEvents?.some((r) => !r.deletion && r.event.payload.action.body)}
        <details>
          <summary>Your saved post history</summary
          >{#each profile.ownedEvents as record (record.event.id)}
            {#if !record.deletion && record.event.payload.action.body}<p>
                {record.event.payload.action.headline || ''}
                {record.event.payload.action.body}
              </p>{/if}
          {/each}
        </details>
      {/if}
      <div class="form-actions form-actions--footer"><ActionButton onclick={() => openMode('reset')}>Reset this browser</ActionButton></div>
      </div>
    {:else}
      <form id={id + '-form'} class="task-form" bind:this={form} onsubmit={submit}>
        {#if mode === 'reset'}
          <p>
            This clears your account, messages and unsent posts from this browser. Save a backup
            first if you want to return.
          </p>
          <label class="check"
            ><input type="checkbox" required bind:checked={confirmed} />Clear my local data</label
          >
        {:else}
          {#if mode === 'import'}
            <div class="form-field">
              <label for={id + '-file'}>Backup file (.age)</label><input
                id={id + '-file'}
                type="file"
                accept=".age,application/octet-stream"
                required
                onchange={(e) => (file = e.currentTarget.files?.[0])}
              />
            </div>
          {:else}<p>
              Includes your posts, profile and unsent drafts. Private messages stay on this device.
            </p>{/if}
          <div class="form-field">
            <label for={id + '-passphrase'}>Backup passphrase</label><input
              id={id + '-passphrase'}
              type="password"
              required
              minlength="12"
              autocomplete={mode === 'import' ? 'current-password' : 'new-password'}
              aria-describedby={mode === 'export' ? id + '-passphrase-help' : undefined}
              oninput={() => repeatError = ''}
              readonly={busy}
              bind:value={phrase}
            />
          {#if mode === 'export'}
            <p class="form-help" id={id + '-passphrase-help'}>
              Use at least 12 characters. You’ll need this passphrase to open the file.
            </p>
          {/if}
          </div>
          {#if mode === 'export'}
            <div class="form-field">
              <label for={id + '-confirm'}>Confirm passphrase</label><input
                id={id + '-confirm'}
                type="password"
                required
                autocomplete="new-password"
                aria-invalid={repeatError ? true : undefined}
                aria-describedby={repeatError ? id + '-confirm-error' : undefined}
                oninput={() => repeatError = ''}
                readonly={busy}
                bind:this={confirmation}
                bind:value={repeat}
              />
              {#if repeatError}<p class="form-error" id={id + '-confirm-error'}>{repeatError}</p>{/if}
            </div>
            <label class="check"
              ><input type="checkbox" bind:checked={includeHome} />Include older posts from my home</label
            >
          {:else}
            <details class="account-disclosure">
              <summary>Move to a different home</summary>
              <div class="form-field">
                <label for={id + '-invitation'}>New home invitation</label>
                <textarea id={id + '-invitation'} bind:value={invite} spellcheck="false" autocomplete="off" readonly={busy}></textarea>
              </div>
            </details>
            <label class="check"
              ><input type="checkbox" bind:checked={drafts} />Keep unsent posts as drafts</label
            >
            <p class="restore-consequence">
              Restoring makes this browser your account’s active device. Your previous devices will
              need to reconnect. Existing chats on this browser will be cleared.
            </p>
            <label class="check"
              ><input type="checkbox" required bind:checked={confirmed} />Make this my active device</label
            >
          {/if}
        {/if}
      </form>
    {/if}
  {/snippet}
</Sheet>

{#snippet buttons(close: () => void)}
  {#if error}<p class="form-feedback form-error action-feedback" role="alert">{error}</p>{/if}
  <div class="form-actions account-actions">
    <ActionButton type="button" disabled={busy} onclick={() => (restoreOnly ? close() : openMode('overview'))}>Cancel</ActionButton>
    <ActionButton form={id + '-form'} variant={mode === 'reset' ? 'danger' : 'filled'} type="submit" disabled={busy}>
      {busy ? 'Working…' : mode === 'export' ? 'Download backup' : mode === 'import' ? 'Restore account' : 'Reset browser'}
    </ActionButton>
  </div>
{/snippet}

<style>
  .account-overview h3 { margin-block: .5rem 0; }
  .account-disclosure { border-block-start: 1px solid var(--line); }
  .restore-consequence { font: var(--type-small); }
  .account-actions { justify-content: flex-end; }
  .action-feedback { margin: 0 0 .5rem; }
</style>
