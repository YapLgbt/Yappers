<script lang="ts">
  import { tick } from 'svelte';
  import ActionButton from './ActionButton.svelte';
  import type { Community, ReportSubject } from './community';
  let { client }: { client: Community } = $props();
  let dialog: HTMLDialogElement;
  let title: HTMLHeadingElement;
  let fields: HTMLDivElement;
  let subject = $state<ReportSubject>('account'), reference = $state(''), include = $state(false);
  let excerpt = $state(''), category = $state('other'), receipt = $state(''), home = $state('');
  let messageText = $state('');
  let phase = $state<'edit'|'review'|'lookup'|'done'>('edit'), busy = $state(false), error = $state(''), outcome = $state('');
  const categories = {harassment:'Harassment', outing:'Outing', stalking:'Stalking', threat:'Threat', intimateImage:'Nonconsensual intimate image', other:'Other'};
  const statuses: Record<string,string> = {received:'Received. Awaiting review.',actionTaken:'Reviewed. Your home took action.',noAction:'Reviewed. Your home closed the case without action.',expired:'The evidence expired before the case was closed.'};
  export function open(kind: ReportSubject = 'account', id = '', text = '', lookup = false) {
    subject = kind; reference = id; include = false; category = 'other';
    messageText = kind === 'message' ? text.slice(0, 2000) : '';
    excerpt = kind === 'message' ? '' : text.slice(0, 2000);
    receipt = ''; home = ''; error = ''; outcome = ''; phase = lookup ? 'lookup' : 'edit';
    dialog.showModal();
  }
  function clear() { excerpt = ''; messageText = ''; reference = ''; receipt = ''; home = ''; error = ''; }
  function close() { if (!busy) dialog.close(); }
  async function changePhase(next: typeof phase) {
    phase = next;
    await tick();
    fields.scrollTop = 0;
    title.focus();
  }
  async function next(event: SubmitEvent) {
    event.preventDefault();
    if (busy || phase === 'done' || (phase === 'edit' && !excerpt.trim() && !include) || (phase === 'lookup' && !receipt.trim())) return;
    busy = true; error = '';
    try {
      if (phase === 'edit') {
        const draft = await client.reportDraft();
        home = draft.home; receipt = draft.receipt; await changePhase('review');
      } else {
        const result = phase === 'lookup' ? await client.reportStatus(receipt.trim()) :
          await client.submitReport(receipt,{subject,category,text:excerpt,reference:include ? reference : null});
        outcome = statuses[result.status] || 'Unknown case status';
        excerpt = ''; messageText = ''; reference = ''; await changePhase('done');
      }
    } catch (failure) {
      const reason = String(failure);
      error = phase === 'review' && reason.includes('remove recovery keys') ? 'Edit your excerpt to remove recovery keys and private receipts before sending.' :
        phase === 'review' && reason.includes('draft expired') ? 'This draft expired. Choose Edit and review it again for a new receipt.' :
        phase === 'review' && reason.includes('inbox is full') ? 'Your home’s inbox is full. Try again later.' :
        phase === 'lookup' ? 'Could not retrieve this case. Check your home, connection and receipt. Receipts expire after 30 days.' :
        phase === 'review' ? 'Delivery was not confirmed. Keep this receipt to check the case or retry the same report.' :
        'Your primary home’s report inbox is unavailable. No evidence was sent.';
    } finally { busy = false; }
  }
</script>
<dialog id="report-dialog" bind:this={dialog} onclose={clear} oncancel={event => { if (busy) event.preventDefault(); }} aria-labelledby="report-title">
  <div class="dialog-title"><h2 id="report-title" bind:this={title} tabindex="-1">{phase === 'lookup' || phase === 'done' ? 'Report status' : 'Report to your home'}</h2><ActionButton type="button" disabled={busy} onclick={close}>Done</ActionButton></div>
  <form class="task-form report-form" onsubmit={next}>
  <div class="task-form report-fields" bind:this={fields}>
  {#if phase === 'edit'}
    <div class="form-field">
      <label for="report-reason">Reason</label>
      <select id="report-reason" disabled={busy} bind:value={category}>{#each Object.entries(categories) as [key,label]}<option value={key}>{label}</option>{/each}</select>
    </div>
    <div class="form-field">
      <label for="report-evidence">Evidence for the moderator</label>
      <textarea id="report-evidence" maxlength="2000" rows="4" autocomplete="off" spellcheck={false} readonly={busy} bind:value={excerpt} aria-describedby="report-evidence-help"></textarea>
      <div class="evidence-support">
        <p class="form-help" id="report-evidence-help">Edit or remove details you want to keep private. Keep recovery keys private. Your moderator can read any private-message text you include.</p>
        <span class="form-help evidence-count" aria-hidden="true">{excerpt.length} / 2000</span>
      </div>
    </div>
    {#if messageText && !excerpt}<div class="form-actions"><ActionButton type="button" disabled={busy} onclick={() => excerpt = messageText}>Use this message’s text</ActionButton></div>{/if}
    {#if reference}<label class="check"><input type="checkbox" bind:checked={include}> Include {subject === 'message' || subject === 'account' ? 'account' : subject} ID</label>
      {#if include}<p class="identifier">{reference}</p>{/if}{/if}
  {:else if phase === 'review'}
    <p>Your primary home’s moderator will receive the following {subject} report. Deleting the original leaves this report in place. Yap’s software clears the evidence on closure or after seven days. Your operator can copy it.</p>
    <dl class="report-summary">
      <div><dt>Receiving home</dt><dd class="identifier">{home}</dd></div>
      <div><dt>Reason</dt><dd>{categories[category as keyof typeof categories]}</dd></div>
      {#if include}<div><dt>Subject ID</dt><dd class="identifier">{reference}</dd></div>{/if}
      {#if excerpt}<div><dt>Evidence for the moderator</dt><dd class="excerpt">{excerpt}</dd></div>{/if}
    </dl>
  {:else if phase === 'done'}
    <p class="form-feedback" role="status">{outcome}</p>
  {/if}
  {#if phase !== 'edit'}
    <div class="form-field">
      <label for="report-receipt">Private receipt</label>
      <textarea id="report-receipt" rows="3" bind:value={receipt} autocomplete="off" spellcheck={false} readonly={busy || phase !== 'lookup'} aria-describedby={phase === 'lookup' ? undefined : 'report-receipt-help'}></textarea>
      {#if phase !== 'lookup'}
        <p class="form-help" id="report-receipt-help">{phase === 'review' ? 'Save this receipt to check the outcome for 30 days. Anyone holding it can read the status.' : 'Keep this receipt to check the case in Preferences.'}</p>
      {/if}
    </div>
  {/if}
  </div>
  {#if phase !== 'done'}
    <div class="report-footer">
      {#if error}<p class="form-feedback form-error" role="alert">{error}</p>{/if}
      <div class="form-actions form-actions--footer">
        {#if phase === 'review'}<ActionButton type="button" disabled={busy} onclick={() => {error=''; void changePhase('edit');}}>Edit</ActionButton>{/if}
        <ActionButton type="submit" variant="filled" disabled={busy || (phase === 'edit' && !excerpt.trim() && !include) || (phase === 'lookup' && !receipt.trim())}>
          {busy ? 'Working…' : phase === 'edit' ? 'Review report' : phase === 'review' ? 'Send these details' : 'Check status'}
        </ActionButton>
      </div>
    </div>
  {/if}
  </form>
</dialog>
<style>
  #report-title:focus { outline: none; }
  .report-form { margin-block-start: .625rem; }
  .report-fields { padding: .375rem .375rem .625rem; margin-inline: -.375rem; }
  .report-footer > p { margin: 0 0 .5rem; }
  .identifier, .excerpt { overflow-wrap: anywhere; }
  .excerpt { white-space: pre-wrap; }
  .evidence-support { display: flex; flex-wrap: wrap; gap: .125rem .75rem; }
  .evidence-support p { flex: 1 1 12rem; }
  .evidence-count { margin-inline-start: auto; white-space: nowrap; font-variant-numeric: tabular-nums; }
  .report-summary { display: grid; gap: .75rem; padding-block: 1rem; border-block: 1px solid var(--mtrl-sys-color-outline-variant); }
  .report-summary dt { font: var(--type-label); }
  .report-summary dd { margin: .25rem 0 0; }
  @media (min-height: 480px) {
    #report-dialog[open] { display: flex; flex-direction: column; overflow: clip; }
    #report-dialog > .dialog-title, .report-footer { flex: none; }
    .report-form { display: flex; flex-direction: column; min-block-size: 0; }
    .report-fields { overflow-y: auto; overscroll-behavior: contain; scroll-padding-block: .625rem; }
  }
</style>
