<script lang="ts">
  import ActionButton from './ActionButton.svelte';
  import { tick } from 'svelte';
  import { errorMessage, type Community } from './community';
  let { client, domain, inline = false, connecting = false, onclaimed }: {
    client: Community; domain: string | null; inline?: boolean; connecting?: boolean; onclaimed?: () => Promise<void>;
  } = $props();
  let prefix = $derived(inline ? 'welcome-username' : 'username');
  let dialog = $state<HTMLDialogElement>();
  let field: HTMLInputElement;
  let name = $state(''), nameError = $state(''), error = $state(''), busy = $state(false);
  export function focus() { field.focus(); }
  export async function open() {
    name = ''; nameError = ''; error = ''; dialog?.showModal(); await tick(); field.focus();
  }
  async function claim(event: SubmitEvent) {
    event.preventDefault(); if (busy || (inline && !domain)) return; busy = true; error = '';
    try {
      await client.claimUsername(name.trim().toLowerCase());
      if (onclaimed) await onclaimed(); else dialog?.close();
    }
    catch (problem) {
      error = String(problem).includes('That username is taken.') ? 'That username is taken. Try another.' : errorMessage(problem);
    }
    finally { busy = false; }
  }
</script>
{#snippet claimForm()}
  <form class="task-form username-form" id={prefix + '-form'} autocomplete="off" onsubmit={claim}>
    <p>This permanent address appears under your display name on named posts and replies.</p>
    <div class="form-field">
      <label for={prefix + '-local'}>{domain ? 'Username on ' : 'Username'}<span id={prefix + '-domain'}>{domain ?? ''}</span></label>
      <input id={prefix + '-local'} bind:value={name} bind:this={field} required maxlength="24"
        pattern={'[A-Za-z][A-Za-z0-9_]{1,23}'} name="public-handle" autocomplete="off" data-1p-ignore data-bwignore="true" data-lpignore="true" autocapitalize="none" spellcheck="false" readonly={busy}
        aria-invalid={nameError ? true : undefined} aria-describedby={nameError ? prefix + '-field-error ' + prefix + '-hint' : prefix + '-hint'}
        oninput={() => { nameError = ''; error = ''; }}
        oninvalid={() => { nameError = field.validity.valueMissing ? 'Enter a username.' : 'Use the format below.'; }}>
      {#if nameError}<p class="form-error" id={prefix + '-field-error'}>{nameError}</p>{/if}
      <p id={prefix + '-hint'} class="form-help">Use 2–24 letters, numbers, or underscores. Start with a letter.</p>
    </div>
    {#if error}<p id={prefix + '-error'} class="form-feedback form-error" role="alert">{error}</p>{/if}
    <div class="form-actions form-actions--footer">
      {#if !inline}<ActionButton type="button" data-close disabled={busy} onclick={() => dialog?.close()}>Cancel</ActionButton>{/if}
      <ActionButton type="submit" variant="filled" disabled={busy || (inline && (!domain || !/^[a-z][a-z0-9_]{1,23}$/i.test(name.trim())))}>{connecting ? 'Connecting…' : busy ? 'Claiming…' : 'Claim username'}</ActionButton>
    </div>
  </form>
{/snippet}
{#if inline}
  {@render claimForm()}
{:else}
<dialog id="username-dialog" bind:this={dialog} aria-labelledby="username-title" oncancel={event => { if (busy) event.preventDefault(); }}>
  <div class="dialog-title"><h2 id="username-title">Choose a username</h2></div>
    {@render claimForm()}
  </dialog>
{/if}

<style>
  .username-form { margin-block-start: .625rem; }
</style>
