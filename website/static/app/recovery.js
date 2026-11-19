import {start, profile, modify, confirmAccount, restoreIdentity, canRestoreHere} from './network.js';
import {browser_recovery_key, read_browser_recovery_key} from './runtime/yap.js';
import {createRecoveryFile, readRecoveryFile} from './passkeys.js';

export function bindRecovery(dialog) {
  const el = id => dialog.querySelector('#'+id);
  let session;
  let downloadUrl;

  function message(text) { el('recovery-status').textContent = text; }
  function lockControls(busy) {
    for (const id of ['copy-key','save-managed-key','get-managed-key','reveal-key','protect-account','restore-account','use-key']) el(id).disabled = busy;
  }
  async function perform(action) {
    const current = session;
    lockControls(true);
    message('');
    try {
      await action(current);
    } catch (error) {
      if (current !== session) return;
      message(error.name === 'NotAllowedError' ? 'Passkey verification was cancelled or unavailable. You can try again.' : error.message);
    } finally {
      if (current === session && dialog.open) lockControls(false);
    }
  }
  function clearSecrets() {
    session = undefined;
    el('backup-key').value = '';
    el('backup-key').type = 'password';
    el('restore-key').value = '';
    el('recovery-file').value = '';
    el('reveal-key').textContent = 'Show';
    el('reveal-key').setAttribute('aria-pressed','false');
    el('download-recovery').hidden = true;
    el('download-recovery').removeAttribute('href');
    if (downloadUrl) URL.revokeObjectURL(downloadUrl);
    downloadUrl = undefined;
  }
  dialog.addEventListener('close',clearSecrets);

  function openRecovery(mode) {
    if (dialog.open || mode === 'backup' && !profile) return;
    clearSecrets();
    session = {};
    const hasManager = typeof window.PasswordCredential === 'function' && !!navigator.credentials?.store;
    el('save-managed-key').hidden = !hasManager;
    el('get-managed-key').hidden = !hasManager;
    const backup = mode === 'backup';
    const eligible = !profile || canRestoreHere();
    el('backup-panel').hidden = !backup;
    el('restore-panel').hidden = backup || !eligible;
    el('recovery-heading').textContent = backup ? 'Back up account' : 'Restore an account';
    dialog.querySelector('details').open = false;
    message('');
    dialog.showModal();
    lockControls(false);
    if (!backup) {
      if (eligible) el('restore-key').focus();
      else message('This browser already has an account in use. Open Yap in a fresh browser profile to restore another account.');
      return;
    }
    perform(async current => {
      message('Getting your recovery key…');
      const wallet = await confirmAccount();
      if (current !== session) return;
      if (wallet.deviceSecret !== profile.wallet.deviceSecret) throw new Error('Your account changed. Reopen backup to try again.');
      current.device = wallet.deviceSecret;
      el('backup-key').value = browser_recovery_key(JSON.stringify(wallet));
      message('');
    });
  }
  async function rememberBackup(current) {
    await modify(state => {
      if (state.wallet.deviceSecret !== current.device) throw new Error('Your account changed. Reopen backup to try again.');
      return {...state,recoveryKeyIssued:true};
    });
  }
  function revealKey(show) {
    el('backup-key').type = show ? 'text' : 'password';
    el('reveal-key').textContent = show ? 'Hide' : 'Show';
    el('reveal-key').setAttribute('aria-pressed',String(show));
  }
  el('reveal-key').onclick = () => perform(async current => {
    if (!el('backup-key').value) throw new Error('Connect to your home, then reopen Back up account.');
    const show = el('backup-key').type === 'password';
    if (show) await rememberBackup(current);
    if (current === session) revealKey(show);
  });
  el('copy-key').onclick = () => perform(async current => {
    if (!el('backup-key').value) {
      message('Connect to your home, then reopen Back up account.');
      return;
    }
    try {
      await navigator.clipboard.writeText(el('backup-key').value);
      if (current !== session) return;
      message('Copied. Paste it into a secure note in your password manager.');
    } catch {
      if (current !== session) return;
      revealKey(true);
      el('backup-key').focus();
      el('backup-key').select();
      message('Copy the selected key and save it in your password manager.');
    }
    await rememberBackup(current);
  });
  const credentialPrefix = 'Yap recovery · ';
  el('save-managed-key').onclick = () => perform(async current => {
    const key = el('backup-key').value;
    if (!key) throw new Error('Reopen Back up account to get your key.');
    const account = profile.wallet.chain.at(-1).payload.account;
    try {
      await navigator.credentials.store(new PasswordCredential({
        id:credentialPrefix+account, name:(profile.alias || 'Yap')+' recovery key', password:key
      }));
    } catch {
      if (current === session) message('Couldn’t open your password manager. You can still copy your key.');
      return;
    }
    if (current !== session) return;
    await rememberBackup(current);
    if (current === session) message('Check your password manager for the Yap recovery key.');
  });
  el('get-managed-key').onclick = () => perform(async current => {
    let entry;
    try { entry=await navigator.credentials.get({password:true,mediation:'required'}); }
    catch { if (current === session) message('You can paste your recovery key below.'); return; }
    if (current !== session) return;
    if (!entry || entry.type !== 'password' || !entry.id.startsWith(credentialPrefix)) {
      message('No Yap recovery key selected. You can paste one below.');
      return;
    }
    el('restore-key').value=entry.password;
    message('Key selected. Choose Restore account to continue.');
  });
  el('protect-account').onclick = () => perform(async current => {
    if (!el('backup-key').value) throw new Error('Connect to your home, then reopen Back up account.');
    const wallet = profile.wallet;
    const file = await createRecoveryFile(wallet);
    await rememberBackup(current);
    if (current !== session) return;
    if (downloadUrl) URL.revokeObjectURL(downloadUrl);
    downloadUrl = URL.createObjectURL(file);
    const link = el('download-recovery');
    link.href = downloadUrl;
    link.download = 'yap-'+wallet.chain.at(-1).payload.account.slice(0,8)+'.yap-recovery';
    link.hidden = false;
    message('Your file is ready. Save it somewhere you can reach from another device.');
    link.focus();
  });
  async function restore(identity, current) {
    if (current !== session) return;
    message('Opening your account…');
    await restoreIdentity(identity);
    location.reload();
  }
  el('key-form').onsubmit = event => {
    event.preventDefault();
    perform(async current => {
      if (!profile || profile.welcomePending) await start('restore');
      const identity = JSON.parse(read_browser_recovery_key(el('restore-key').value,profile.join.serverAddr.id));
      return restore(identity,current);
    });
  };
  el('restore-account').onclick = () => {
    el('recovery-file').value = '';
    el('recovery-file').click();
  };
  el('recovery-file').onchange = () => perform(async current => {
    const file = el('recovery-file').files[0];
    if (!file) return;
    const identity = await readRecoveryFile(file);
    if (!profile || profile.welcomePending) await start('restore');
    await restore(identity,current);
  });

  return openRecovery;
}
