<script lang="ts" module>
  export type WelcomeStep = 'intro' | 'restore' | 'name' | 'returning' | 'username' | 'backup' | null;
</script>
<script lang="ts">
  import ActionButton from './ActionButton.svelte';
  import Username from './Username.svelte';
  import AccountData from './AccountData.svelte';
  import type {Community} from './community';
  import { tick } from 'svelte';
  import { errorMessage, type RecoveryMode } from './community';
  import { pageMotion } from './pageMotion';
  import { suggestName } from './names';
  const steps = ['intro', 'returning', 'restore', 'name', 'username', 'backup'];
  let { client, step = $bindable(), create, enter, recovery }: {
    client:Community|null; step: WelcomeStep; create: (alias: string,invitation?:string) => Promise<void>; enter: () => Promise<void>;
    recovery: (mode: RecoveryMode) => void;
  } = $props();
  let alias = $state(''), nameError = $state(''), message = $state(''), busy = $state(false), failed = $state(false);
  let invitation=$state(''), joining = $state(false), newAccount = $state(false);
  let handleForm = $state<Username>();
  let domain = $state(''), checking = $state(false), checkError = $state('');
  async function verified(home: Community) {
    if (home.profile?.setupStep === 'restored-username') await enter();
    else await go('backup');
  }
  async function checkUsername(home: Community) {
    checking = true; checkError = ''; domain = '';
    try {
      const status = await home.verifyWelcomeUsername();
      if (status.username) await verified(home);
      else domain = status.domain;
    } catch (problem) {
      checkError = errorMessage(problem);
    } finally { checking = false; }
  }
  $effect(() => {
    if (step === 'username' && client && !joining) void checkUsername(client);
  });
  let field = $state<HTMLInputElement>();
  let heading: HTMLHeadingElement;
  async function go(next: WelcomeStep) {
    step = next; message = ''; nameError = ''; failed = false;
    await tick();
    if (next) window.scrollTo({top: 0, behavior: 'instant'});
    if (next === 'username' && newAccount) handleForm?.focus();
    else if (next) (next === 'name' ? field : heading)?.focus();
  }
  async function join(event: SubmitEvent) {
    event.preventDefault();
    if (busy || joining) return;
    const name = alias.trim();
    if (name.length < 2) {
      nameError = name ? 'Use at least two characters for your display name.' : 'Choose a display name to continue.';
      await tick();
      field?.focus();
      return;
    }
    failed = false;
    joining = true;
    newAccount = true;
    domain = '';
    checkError = '';
    try {
      await go('username');
      await create(name, invitation);
      invitation = '';
    } catch (problem) {
      newAccount = false;
      await go('name');
      failed = true;
      message = errorMessage(problem);
    } finally { joining = false; }
  }
  async function finish() {
    if (busy) return;
    busy = true;
    failed = false;
    try { await enter(); message = ''; }
    catch (problem) { failed = true; message = errorMessage(problem); }
    finally { busy = false; }
  }
</script>
<main id="welcome" class:initial={step === "intro"} aria-labelledby="welcome-title" hidden={step === null}>
{#if step === 'intro'}
  <a class="welcome-brand" href="/" aria-label="Yap home"><img src="/assets/yap-wordmark.svg" width="1745" height="773" alt="Yap"></a>
{/if}
<section class="welcome-content" use:pageMotion={step ? steps.indexOf(step) : null}><h1 class:sr-only={step === 'intro'} id="welcome-title" bind:this={heading} tabindex="-1">{step === "restore" ? "Restore your account" : step === "username" ? "Choose a username" : step === "backup" ? "Keep your account" : step === "name" ? "What should neighbors call you?" : "Welcome to Yap"}</h1>
<div id="welcome-intro" hidden={step !== "intro"}><p>Yap is a neighborhood for queer conversation. Join yap.lgbt to share a plan, ask a question or find your people.</p>
<div class="welcome-primary-actions">
  <ActionButton type="button" variant="filled" onclick={() => go("name")} id="welcome-create">Create an account</ActionButton>
  <ActionButton type="button" variant="outlined" onclick={() => go("restore")} id="welcome-restore-existing">Restore an existing account</ActionButton>
</div>
<a class="demo-link" href="/app/demo/">Try the local demo</a></div>
{#if step === 'restore'}
<div class="welcome-restore-actions">
  <ActionButton type="button" variant="tonal" onclick={() => recovery("restore")} id="welcome-restore">Use a recovery key or passkey</ActionButton>
  {#if client}<AccountData {client} profile={null} restoreOnly/>{/if}
  <ActionButton type="button" onclick={() => go("intro")}>Back</ActionButton>
</div>
{/if}
<div id="welcome-returning" hidden={step !== "returning"}><p>Your account is already on this device. You can post by name or anonymously in your neighborhood.</p>
<ActionButton type="button" variant="filled" onclick={() => go("backup")} id="guide-continue">Continue with my account</ActionButton>
<ActionButton type="button" onclick={() => go(null)} id="guide-exit">Return to neighborhood</ActionButton></div>
{#if step === 'name'}
<form class="task-form" id="welcome-name" novalidate autocomplete="off" onsubmit={join}>
  <p id="welcome-name-help">Use a name you’re comfortable posting with. You can also post anonymously.</p>
  <div class="form-field">
    <label for="welcome-alias">Display name (required)</label>
    <input id="welcome-alias" bind:value={alias} bind:this={field} required minlength="2" maxlength="30" name="display-name" autocomplete="off" enterkeyhint="next" data-1p-ignore data-bwignore="true" data-lpignore="true" readonly={busy} aria-invalid={nameError ? true : undefined} aria-describedby={nameError ? 'welcome-alias-error welcome-name-help' : 'welcome-name-help'} oninput={() => nameError = ''}>
    {#if nameError}<p class="form-error" id="welcome-alias-error">{nameError}</p>{/if}
    <div class="form-actions"><ActionButton onclick={() => { alias = suggestName(alias); nameError = ''; field?.focus(); field?.select(); }} id="suggest-name" type="button" disabled={busy}>Suggest a name</ActionButton></div>
  </div>
  <details class="home-invitation"><summary>Have an invitation to another home?</summary>
    <div class="form-field">
      <label for="welcome-invitation">Home invitation</label>
      <textarea id="welcome-invitation" spellcheck="false" autocomplete="off" readonly={busy} bind:value={invitation}></textarea>
    </div>
  </details>
  <div class="form-actions form-actions--footer">
    <ActionButton type="button" disabled={busy} onclick={() => go("intro")} id="welcome-back">Back</ActionButton>
    <ActionButton type="submit" variant="filled" disabled={busy} id="welcome-join">Continue</ActionButton>
  </div>
</form>
{/if}
{#if step === 'username'}
  {#if client && (newAccount || domain)}
    <Username bind:this={handleForm} {client} domain={domain || null} inline connecting={joining || checking} onclaimed={() => verified(client!)} />
  {/if}
  {#if checkError}
    <p class="form-feedback form-error" role="alert">{checkError}</p>
    <ActionButton type="button" disabled={checking || !client} onclick={() => client && checkUsername(client)}>Try again</ActionButton>
  {:else if checking && !newAccount}
    <p role="status">Checking your username with your home…</p>
  {/if}
{/if}
<div id="welcome-backup" hidden={step !== "backup"}><p>Your account is ready. Save a recovery key in your password manager to use it on another device.</p>
<div class="actions"><ActionButton type="button" onclick={() => recovery("backup")} id="welcome-get-key">Back up account</ActionButton><ActionButton type="button" variant="filled" disabled={busy} onclick={finish} id="welcome-enter">Enter neighborhood</ActionButton></div>
<p class="hint">You can back up later in Preferences.</p></div>
<p id="welcome-error" class="form-feedback" class:form-error={failed} role="status">{message}</p></section>
  <figure class="flag-print welcome-art">
    <div class="cloth" aria-hidden="true">
      <img src="/app/estuary.webp" width="660" height="400" alt="">
      <canvas hidden aria-hidden="true"></canvas>
    </div>
    <figcaption>
      <span data-flag-name>Rainbow Estuary</span>
    </figcaption>
  </figure>
  {#if step === 'intro'}
    <footer class="welcome-footer"><p>For ages 18 and older.</p><nav aria-label="Policies">
      <a href="/terms/">Terms of Service</a><a href="/privacy-policy/">Privacy Policy</a>
    </nav></footer>
  {/if}
</main>

<style>
  #welcome.initial {
    min-height: 100dvh;
    max-width: 36rem;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto auto auto 1fr;
    align-content: start;
    padding-block-start: clamp(1.75rem, 5dvh, 3.5rem);
    gap: clamp(.75rem, 2.4dvh, 1.5rem);
  }
  .welcome-brand { justify-self: center; }
  .welcome-brand img { display: block; width: clamp(8rem, 33vw, 10.75rem); height: auto; }
  @media (prefers-color-scheme: dark) { .welcome-brand img { filter: brightness(0) invert(96%); } }
  .initial .welcome-art { grid-row: 2; }
  .initial .welcome-art .cloth { aspect-ratio: 2.45; }
  .initial .welcome-art figcaption { min-height: 1.75rem; }
  .initial .welcome-content { text-align: center; max-width: none; }
  #welcome-intro > p { margin: 0 auto 1.25rem; }
  .welcome-primary-actions, .welcome-restore-actions { display: grid; gap: .625rem; }
  .welcome-primary-actions :global(button) { min-height: 3rem; }
  .demo-link { display: inline-flex; align-items: center; min-height: 2.75rem; margin-block-start: .25rem; }
  .welcome-footer { align-self: end; text-align: center; font: var(--type-small); color: var(--quiet); }
  .welcome-footer p { margin: 0; }
  .welcome-footer nav { display: flex; flex-wrap: wrap; justify-content: center; gap: 0 1.25rem; }
  .welcome-footer a { display: inline-flex; align-items: center; min-height: 2.75rem; }
  #welcome-name { margin-block-start: .875rem; }
  #welcome-name > p { margin: 0; }
  #welcome-name .home-invitation { margin: 0; }
  #welcome-name .form-field .form-actions { margin-block-start: .375rem; }
  #welcome-error { color: var(--mtrl-sys-color-on-secondary-container); }
  #welcome-error.form-error { color: var(--mtrl-sys-color-on-error-container); }
</style>
