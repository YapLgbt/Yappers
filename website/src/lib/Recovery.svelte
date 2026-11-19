<script lang="ts">
  import ActionButton from './ActionButton.svelte';
  import { loadRecovery, type RecoveryMode } from './community';
  let dialog: HTMLDialogElement;
  let binding: ReturnType<typeof loadRecovery> | undefined;
  export async function open(mode: RecoveryMode) {
    binding ??= loadRecovery(dialog);
    try { (await binding)(mode); } catch (error) { binding = undefined; throw error; }
  }
</script>
<dialog bind:this={dialog} id="recovery" aria-labelledby="recovery-heading">
  <div class="dialog-title"><h2 id="recovery-heading">Back up account</h2><ActionButton type="button" data-close onclick={() => dialog.close()}>Close</ActionButton></div>
  <section class="task-form" id="backup-panel">
    <p id="backup-key-help">Anyone with this key can use your account. Keep it somewhere private.</p>
    <div class="form-field">
      <label for="backup-key">Recovery key</label>
      <div class="key-row">
        <input id="backup-key" type="password" readonly autocomplete="off" spellcheck="false" aria-describedby="backup-key-help">
        <ActionButton id="reveal-key" type="button" aria-controls="backup-key" aria-pressed="false">Show</ActionButton>
      </div>
    </div>
    <div class="form-actions">
      <ActionButton type="button" variant="filled" id="save-managed-key" hidden>Save to password manager</ActionButton>
      <ActionButton type="button" variant="tonal" id="copy-key">Copy recovery key</ActionButton>
    </div>
    <details>
      <summary>Use a passkey and file instead</summary>
      <p class="form-help">You’ll need both the file and the same passkey to restore.</p>
      <div class="form-actions">
        <ActionButton type="button" id="protect-account" variant="tonal">Create recovery file</ActionButton>
        <a id="download-recovery" class="mtrl-button mtrl-button--outlined" hidden download="yap-account.yap-recovery">Save recovery file</a>
      </div>
    </details>
  </section>
  <section class="task-form" id="restore-panel" hidden>
    <form class="task-form" id="key-form" method="dialog">
      <div class="form-field">
        <label for="restore-key">Recovery key</label>
        <input id="restore-key" type="password" required maxlength="128" autocomplete="off" autocapitalize="none" spellcheck="false" aria-describedby="restore-hint">
        <p class="form-help" id="restore-hint">Paste the key from your password manager to open your account on this device.</p>
      </div>
      <div class="form-actions">
        <ActionButton variant="filled" type="submit" id="use-key">Restore account</ActionButton>
        <ActionButton type="button" id="get-managed-key" hidden>Use a saved key</ActionButton>
      </div>
    </form>
    <div class="form-actions form-actions--footer restore-alternative">
      <ActionButton id="restore-account" type="button">Use a recovery file</ActionButton>
      <input id="recovery-file" type="file" accept=".yap-recovery,.json,application/json" hidden>
    </div>
  </section>
  <p id="recovery-status" class="form-feedback" role="status"></p>
</dialog>

<style>
  #recovery > section { margin-block-start: .625rem; }
  #recovery .key-row { margin-block-start: .375rem; }
  #backup-key { margin: 0; }
  #recovery .form-actions :global(.mtrl-button--filled) { margin: 0; }
  #recovery details .form-actions { margin-block-start: .5rem; }
  #recovery .restore-alternative { justify-content: flex-start; }
  #download-recovery { margin: 0; text-decoration: none; }
  #recovery-status { margin: .875rem 0 0; }
  #recovery-status:empty { display: block; margin: 0; padding: 0; }
</style>
