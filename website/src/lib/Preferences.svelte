<script lang="ts">
  import PrivacySwitch from './PrivacySwitch.svelte';
  import ActionButton from './ActionButton.svelte';
  import type { Profile, BlockedAccount, RecoveryMode, Privacy } from './community';
  import type { Action } from 'svelte/action';
  let { profile, domain, discard, reports, chooseUsername, recovery, setPrivacy, unmute, refresh, guide, unblock, pull }: {
    discard:(ids:string[])=>Promise<void>; reports: (lookup:boolean)=>void; profile: Profile; domain: string | null; chooseUsername: () => void; recovery: (mode: RecoveryMode) => void;
    setPrivacy: (value: Partial<Privacy>) => Promise<void>; unmute:(id:string)=>Promise<void>; refresh: () => void; guide: () => void;
    unblock: (account: BlockedAccount) => void; pull: Action<HTMLElement>;
  } = $props();
  let dialog: HTMLDialogElement;
  let body: HTMLDivElement;
  let launcher: HTMLElement | null = null;
  let removing = $state(false), uploadError = $state('');
  async function discardPending() {
    const ids = profile.outbox.map(event=>event.id);
    if (!ids.length || !window.confirm('Discard '+ids.length+' pending upload'+(ids.length===1?'':'s')+'? Anything already delivered stays on your home.')) return;
    removing=true; uploadError='';
    try { await discard(ids); } catch { uploadError='Couldn’t discard uploads. Try again.'; } finally { removing=false; }
  }
  let showBlocked = $state(false);
  export function open(blocked = false, trigger?: HTMLElement) {
    launcher = trigger ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
    launcher?.focus({ preventScroll: true });
    showBlocked=blocked;
    dialog.showModal();
    if(blocked) requestAnimationFrame(()=>document.getElementById('blocked-heading')?.focus());
    else body.scrollTop=0;
  }
  function openRecovery(mode: RecoveryMode) {
    dialog.close();
    launcher?.focus({ preventScroll: true });
    recovery(mode);
  }
</script>
<dialog bind:this={dialog} id="options" aria-labelledby="options-title">
<div class="dialog-title"><h2 id="options-title">Preferences</h2><ActionButton data-close onclick={() => dialog.close()}>Done</ActionButton></div>
<div class="preferences-body" id="preferences-body" bind:this={body} use:pull>
<section id="profile-username" hidden={!domain}><h3>Your profile</h3><strong id="profile-display-name">{profile.alias}</strong><p class="hint">{profile.username || "Choose a username to post by name."}</p><ActionButton type="button" hidden={!!profile.username} onclick={chooseUsername}>Choose a username</ActionButton></section>
{#if profile.pendingPrivacy || profile.pendingApprovals?.length || profile.pendingUnblocks.length || profile.blocks.some(block=>!block.handle)}
<p role="status">Your changes apply here now and will sync with your home when connected.</p>{/if}
<section aria-labelledby="privacy-title" class="privacy-controls">
<h3 id="privacy-title">Feed and inbox</h3>
<PrivacySwitch id="hide-anonymous" label="Hide anonymous posts and replies" checked={!!profile.hideAnonymous}
  change={hideAnonymous => setPrivacy({hideAnonymous})}/>
{#each [{key:'feedDays' as const,label:'Minimum account age for posts'},{key:'dmDays' as const,label:'Minimum account age for inbox'}] as setting (setting.key)}
  <label>{setting.label}
    <select value={String(profile.inboxPolicy?.[setting.key] || 0)}
      onchange={event => setPrivacy({ [setting.key]: Number(event.currentTarget.value) })}>
      {#each [0,1,7,30] as days (days)}
        <option value={String(days)}>{days === 0 ? 'Any age' : days === 1 ? '1 day' : days + ' days'}</option>
      {/each}
    </select>
  </label>
{/each}
<PrivacySwitch id="approved-contacts" label="Only approved contacts in my inbox" hint="Other messages go to Requests."
  checked={!!profile.inboxPolicy?.approvedOnly} change={approvedOnly => setPrivacy({approvedOnly})}/>
</section>
{#if Object.keys(profile.muted||{}).length}<section><h3>Muted conversations</h3>{#each Object.entries(profile.muted||{}) as [id,title]}<div class="actions"><span>{title}</span><ActionButton onclick={()=>unmute(id)}>Unmute</ActionButton></div>{/each}</section>{/if}
<section aria-labelledby="recovery-title"><h3 id="recovery-title">Account recovery</h3>
<p class="hint">Save your recovery key to use this account on another device.</p>
<div class="actions"><ActionButton type="button" variant="tonal" onclick={() => openRecovery("backup")} id="open-backup">Back up account</ActionButton><ActionButton type="button" onclick={() => openRecovery("restore")} id="open-restore">Restore an account</ActionButton></div>
</section>
{#if profile.outbox.length}<section><h3>Pending uploads</h3>
<p>{profile.outbox.length} waiting to sync</p><ActionButton disabled={removing} onclick={discardPending}>Discard pending uploads</ActionButton></section>{/if}
{#if uploadError}<p role="alert">{uploadError}</p>{/if}
<section><h3>Reports</h3>
<ActionButton onclick={() => {dialog.close();reports(false);}}>Report to your home</ActionButton>
<ActionButton onclick={() => {dialog.close();reports(true);}}>Check a private receipt</ActionButton></section>
<ActionButton type="button" onclick={() => { dialog.close(); refresh(); }} id="refresh-preferences">Refresh posts</ActionButton>
<ActionButton type="button" onclick={() => { dialog.close(); guide(); }} id="replay-guide">Welcome guide</ActionButton>
<p><a href="/source/">Server source</a> · <a href="/peering/">Peering covenant</a> · <a href="/terms/">Terms of Service</a> · <a href="/privacy-policy/">Privacy Policy</a></p>
<section id="blocked-section" hidden={!showBlocked && !profile.blocks.length}><h3 id="blocked-heading" tabindex="-1">Blocked accounts</h3>{#if !profile.blocks.length}<p>No blocked accounts.</p>{/if}<div id="blocked-list">{#each profile.blocks as account (account.event)}<div class="actions"><span>{account.label}</span><ActionButton onclick={() => unblock(account)}>Unblock</ActionButton></div>{/each}</div></section>
</div>
</dialog>

<style>
  #options[open] {
    display: flex;
    flex-direction: column;
    overflow: clip;
  }

  .dialog-title {
    flex: none;
    padding-block-end: .375rem;
    border-block-end: 1px solid var(--line);
  }

  .preferences-body {
    flex: 1 1 auto;
    min-block-size: 0;
    padding: .375rem;
    margin-inline: -.375rem;
    overflow-y: auto;
    overscroll-behavior: contain;
    scroll-padding-block: .375rem;
    scrollbar-width: thin;
    scrollbar-color: var(--outline) transparent;
  }
</style>
